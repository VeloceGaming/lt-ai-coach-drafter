//! The pure decision logic: is this hook call the live draft, and which ranked
//! champion do we answer with. No SDK types in here so it can be tested.
//!
//! The rule is "answer only when sure". Every failed check is a [`Decline`], and a
//! decline means the hook returns `None` — the game's own Head Coach then decides
//! that turn, exactly as if this mod were not installed.

use std::collections::BTreeSet;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::protocol::{self, Recommendation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Blue,
    Red,
}

impl Side {
    pub fn word(self) -> &'static str {
        match self {
            Side::Blue => "blue",
            Side::Red => "red",
        }
    }

    /// From the `in_turn` marker bits the client thread reads: 1 blue, 2 red.
    /// Both lit, or neither, is not a turn we can name.
    pub fn from_turn_bits(bits: u8) -> Option<Side> {
        match bits {
            1 => Some(Side::Blue),
            2 => Some(Side::Red),
            _ => None,
        }
    }

    pub fn from_word(word: &str) -> Option<Side> {
        match word {
            "blue" => Some(Side::Blue),
            "red" => Some(Side::Red),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Ban,
    Pick,
}

/// What the client thread read from the draft screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveBoard {
    /// Every champion tile in the grid.
    pub grid: BTreeSet<String>,
    pub bans: BTreeSet<String>,
    pub blue_picks: BTreeSet<String>,
    pub red_picks: BTreeSet<String>,
    /// Tiles carrying a Fearless marker. Under Hard Fearless the game does not
    /// offer them to the hook; under the soft rule a block is per side, so a tile
    /// marked here may or may not be offered to the side acting. The pool check
    /// therefore accepts either.
    pub blocked: BTreeSet<String>,
}

impl LiveBoard {
    /// The smallest and largest pool the hook may legitimately be offered:
    /// every tile not banned or picked, with and without the Fearless-marked ones.
    pub fn pool_bounds(&self) -> (BTreeSet<String>, BTreeSet<String>) {
        let open: BTreeSet<String> = self
            .grid
            .iter()
            .filter(|tile| {
                !self.bans.contains(*tile)
                    && !self.blue_picks.contains(*tile)
                    && !self.red_picks.contains(*tile)
            })
            .cloned()
            .collect();
        let smallest = open.iter().filter(|tile| !self.blocked.contains(*tile)).cloned().collect();
        (smallest, open)
    }
}

/// What the hook's context says, as champion names.
#[derive(Clone, Debug, Default)]
pub struct HookBoard {
    pub ally_bans: Vec<String>,
    pub enemy_bans: Vec<String>,
    pub ally_picks: Vec<String>,
    pub enemy_picks: Vec<String>,
    pub available: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decline {
    /// The hook's board is not the board on screen (a background simulation, or
    /// the screen moved on).
    BoardMismatch,
    /// The board matches, but the hook's candidate pool is outside the bounds the
    /// screen allows. The counts say which way, which is how a Fearless reading
    /// that is wrong shows up in the log.
    PoolMismatch { extra_in_hook: usize, missing_from_hook: usize },
}

impl HookBoard {
    fn blue_red(&self, acting: Side) -> (&[String], &[String]) {
        match acting {
            Side::Blue => (&self.ally_picks, &self.enemy_picks),
            Side::Red => (&self.enemy_picks, &self.ally_picks),
        }
    }

    /// The Coach's fingerprint, rebuilt from the hook's own lists with the ally
    /// side taken to be `acting`.
    pub fn fingerprint(&self, acting: Side) -> String {
        let bans: Vec<&str> = self
            .ally_bans
            .iter()
            .chain(&self.enemy_bans)
            .map(String::as_str)
            .collect();
        let (blue, red) = self.blue_red(acting);
        let blue: Vec<&str> = blue.iter().map(String::as_str).collect();
        let red: Vec<&str> = red.iter().map(String::as_str).collect();
        protocol::fingerprint(acting.word(), &bans, &blue, &red)
    }
}

/// How many bans and picks the two boards differ by, for near-miss logging:
/// `(ban count difference, pick count difference)`.
pub fn count_gap(live: &LiveBoard, hook: &HookBoard) -> (usize, usize) {
    let bans = (hook.ally_bans.len() + hook.enemy_bans.len()).abs_diff(live.bans.len());
    let picks = (hook.ally_picks.len() + hook.enemy_picks.len())
        .abs_diff(live.blue_picks.len() + live.red_picks.len());
    (bans, picks)
}

/// Checks the hook's board against the screen, independently of the Coach's
/// fingerprint: bans, both pick lists, and the candidate pool must all agree.
pub fn check_board(live: &LiveBoard, hook: &HookBoard, acting: Side) -> Result<(), Decline> {
    let hook_bans: BTreeSet<&String> = hook.ally_bans.iter().chain(&hook.enemy_bans).collect();
    let live_bans: BTreeSet<&String> = live.bans.iter().collect();
    // Lengths too: a duplicated id must not slip through the set comparison.
    if hook_bans != live_bans || hook.ally_bans.len() + hook.enemy_bans.len() != live.bans.len() {
        return Err(Decline::BoardMismatch);
    }
    let (blue, red) = hook.blue_red(acting);
    let as_set = |ids: &[String]| ids.iter().cloned().collect::<BTreeSet<String>>();
    if as_set(blue) != live.blue_picks
        || as_set(red) != live.red_picks
        || blue.len() != live.blue_picks.len()
        || red.len() != live.red_picks.len()
    {
        return Err(Decline::BoardMismatch);
    }
    // The pool must hold every tile that is certainly open and nothing banned,
    // picked or off the grid. Whether the Fearless-marked tiles are in it depends
    // on the rule, so either is accepted. A background simulation's smaller pool
    // fails the lower bound.
    let (smallest, largest) = live.pool_bounds();
    let missing = smallest.difference(&hook.available).count();
    let extra = hook.available.difference(&largest).count();
    if missing > 0 || extra > 0 || hook.available.is_empty() {
        return Err(Decline::PoolMismatch { extra_in_hook: extra, missing_from_hook: missing });
    }
    Ok(())
}

/// The first ranked champion the game still offers.
pub fn choose<'a>(ranked: &'a [String], available: &BTreeSet<String>) -> Option<&'a String> {
    ranked.iter().find(|id| available.contains(*id))
}

/// Whether a decision for `acting` is ours to make: the Coach has named a side and
/// it is that one. The opponent's decisions are never ours, and must never wait.
pub fn is_ours(coach_side: Option<Side>, acting: Side) -> bool {
    coach_side == Some(acting)
}

/// How long the hook may wait for the Coach's answer.
///
/// Bounded by the game's own timer (minus a margin, because the timer's auto-ban
/// does not consult the hooks) and by a hard cap so a draft with no timer cannot
/// hold a worker thread indefinitely.
pub fn wait_budget(timer_seconds: Option<u32>) -> Duration {
    const MARGIN: u32 = 3;
    const CAP: u32 = 12;
    let seconds = match timer_seconds {
        Some(remaining) => remaining.saturating_sub(MARGIN).min(CAP),
        None => CAP,
    };
    Duration::from_secs(u64::from(seconds))
}

/// Reads the seconds out of the timer label (`": 20"`).
pub fn parse_timer(text: &str) -> Option<u32> {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// How often a waiting hook re-checks that the decision is still its to make.
const WAIT_SLICE: Duration = Duration::from_millis(100);

/// Waits until the slot holds a recommendation for `fingerprint`, the deadline
/// passes, or `still_ours` turns false (the Coach went away, or named the other
/// side). Checked before the first wait and every [`WAIT_SLICE`], so a decision
/// that is not ours returns at once.
pub fn wait_for_recommendation(
    slot: &Mutex<Option<Recommendation>>,
    arrived: &Condvar,
    fingerprint: &str,
    deadline: Instant,
    still_ours: &dyn Fn() -> bool,
) -> Option<Recommendation> {
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if !still_ours() {
            return None;
        }
        if let Some(recommendation) = guard.as_ref() {
            if recommendation.fingerprint == fingerprint {
                return Some(recommendation.clone());
            }
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        guard = arrived
            .wait_timeout(guard, (deadline - now).min(WAIT_SLICE))
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn set(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }
    fn v(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// Red's first pick of a delegated draft: two bans, one blue pick.
    fn live() -> LiveBoard {
        LiveBoard {
            grid: set(&["a", "b", "c", "d", "e", "f", "g", "h"]),
            bans: set(&["a", "b"]),
            blue_picks: set(&["c"]),
            red_picks: set(&[]),
            blocked: set(&[]),
        }
    }

    fn hook_for_red() -> HookBoard {
        HookBoard {
            ally_bans: v(&["b"]),
            enemy_bans: v(&["a"]),
            ally_picks: v(&[]),
            enemy_picks: v(&["c"]),
            available: set(&["d", "e", "f", "g", "h"]),
        }
    }

    fn recommendation(fingerprint: &str) -> Recommendation {
        Recommendation { seq: 1, fingerprint: fingerprint.to_string(), bans: v(&["x"]), picks: v(&["y"]) }
    }

    #[test]
    fn the_live_board_passes() {
        assert_eq!(check_board(&live(), &hook_for_red(), Side::Red), Ok(()));
    }

    #[test]
    fn the_fingerprint_is_built_with_the_acting_side_as_ally() {
        assert_eq!(hook_for_red().fingerprint(Side::Red), "red/a,b/c/");
        // The same lists read as blue's would put the pick under red instead.
        assert_eq!(hook_for_red().fingerprint(Side::Blue), "blue/a,b//c");
    }

    #[test]
    fn a_hook_board_acting_for_the_wrong_side_is_refused() {
        assert_eq!(check_board(&live(), &hook_for_red(), Side::Blue), Err(Decline::BoardMismatch));
    }

    #[test]
    fn a_background_simulation_with_a_smaller_pool_is_refused() {
        let mut hook = hook_for_red();
        hook.available = set(&["d", "e", "f"]);
        assert_eq!(
            check_board(&live(), &hook, Side::Red),
            Err(Decline::PoolMismatch { extra_in_hook: 0, missing_from_hook: 2 })
        );
    }

    #[test]
    fn a_pool_offering_a_banned_or_picked_champion_is_refused() {
        let mut hook = hook_for_red();
        hook.available.insert("a".to_string());
        assert_eq!(
            check_board(&live(), &hook, Side::Red),
            Err(Decline::PoolMismatch { extra_in_hook: 1, missing_from_hook: 0 })
        );
    }

    #[test]
    fn a_missing_ban_is_a_board_mismatch() {
        let mut hook = hook_for_red();
        hook.enemy_bans.clear();
        assert_eq!(check_board(&live(), &hook, Side::Red), Err(Decline::BoardMismatch));
    }

    #[test]
    fn hard_fearless_marked_tiles_may_be_absent_from_the_pool() {
        let mut board = live();
        board.blocked = set(&["h"]);
        let mut hook = hook_for_red();
        hook.available = set(&["d", "e", "f", "g"]);
        assert_eq!(check_board(&board, &hook, Side::Red), Ok(()));
    }

    #[test]
    fn soft_fearless_marked_tiles_may_be_present_in_the_pool() {
        // Under the soft rule a champion blue used is still open to red, yet its
        // tile carries a marker. Either reading of the marker must pass.
        let mut board = live();
        board.blocked = set(&["h"]);
        assert_eq!(check_board(&board, &hook_for_red(), Side::Red), Ok(()));
    }

    #[test]
    fn a_marked_tile_does_not_excuse_missing_unmarked_tiles() {
        let mut board = live();
        board.blocked = set(&["h"]);
        let mut hook = hook_for_red();
        hook.available = set(&["d", "e", "f"]);
        assert!(matches!(
            check_board(&board, &hook, Side::Red),
            Err(Decline::PoolMismatch { missing_from_hook: 1, .. })
        ));
    }

    #[test]
    fn near_misses_are_measured_by_count() {
        let mut hook = hook_for_red();
        assert_eq!(count_gap(&live(), &hook), (0, 0));
        hook.enemy_picks.clear();
        assert_eq!(count_gap(&live(), &hook), (0, 1));
    }

    #[test]
    fn choose_takes_the_first_ranked_champion_still_offered() {
        let available = set(&["e", "f"]);
        assert_eq!(choose(&v(&["a", "e", "f"]), &available), Some(&"e".to_string()));
        assert_eq!(choose(&v(&["a", "b"]), &available), None);
        assert_eq!(choose(&[], &available), None);
    }

    #[test]
    fn only_the_side_the_coach_named_is_ours() {
        assert!(is_ours(Some(Side::Red), Side::Red));
        assert!(!is_ours(Some(Side::Red), Side::Blue));
        assert!(!is_ours(None, Side::Red));
    }

    #[test]
    fn the_wait_budget_follows_the_timer_and_is_capped() {
        assert_eq!(wait_budget(None), Duration::from_secs(12));
        assert_eq!(wait_budget(Some(60)), Duration::from_secs(12));
        assert_eq!(wait_budget(Some(20)), Duration::from_secs(12));
        assert_eq!(wait_budget(Some(10)), Duration::from_secs(7));
        assert_eq!(wait_budget(Some(3)), Duration::ZERO);
        assert_eq!(wait_budget(Some(0)), Duration::ZERO);
    }

    #[test]
    fn the_timer_label_is_read() {
        assert_eq!(parse_timer(": 20"), Some(20));
        assert_eq!(parse_timer(": 7"), Some(7));
        assert_eq!(parse_timer(""), None);
    }

    #[test]
    fn only_a_single_lit_turn_marker_names_a_side() {
        assert_eq!(Side::from_turn_bits(1), Some(Side::Blue));
        assert_eq!(Side::from_turn_bits(2), Some(Side::Red));
        assert_eq!(Side::from_turn_bits(0), None);
        assert_eq!(Side::from_turn_bits(3), None);
    }

    #[test]
    fn a_decision_that_is_not_ours_returns_without_waiting() {
        // No recommendation yet and the opponent acting: the hook must not hold a
        // worker thread for the budget.
        let slot = Mutex::new(None);
        let arrived = Condvar::new();
        let started = Instant::now();
        let got = wait_for_recommendation(
            &slot,
            &arrived,
            "blue///",
            Instant::now() + Duration::from_secs(10),
            &|| false,
        );
        assert!(got.is_none());
        assert!(started.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn a_waiting_hook_gives_up_at_the_deadline() {
        let slot = Mutex::new(Some(recommendation("red/old//")));
        let arrived = Condvar::new();
        let started = Instant::now();
        let got = wait_for_recommendation(
            &slot,
            &arrived,
            "red/new//",
            Instant::now() + Duration::from_millis(250),
            &|| true,
        );
        assert!(got.is_none());
        assert!(started.elapsed() >= Duration::from_millis(250));
        assert!(started.elapsed() < Duration::from_millis(600));
    }

    #[test]
    fn a_recommendation_arriving_mid_wait_is_returned() {
        let slot = Arc::new(Mutex::new(Some(recommendation("red/old//"))));
        let arrived = Arc::new(Condvar::new());
        let (slot2, arrived2) = (Arc::clone(&slot), Arc::clone(&arrived));
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            *slot2.lock().unwrap() = Some(recommendation("red/new//"));
            arrived2.notify_all();
        });
        let got = wait_for_recommendation(
            &slot,
            &arrived,
            "red/new//",
            Instant::now() + Duration::from_secs(5),
            &|| true,
        );
        sender.join().unwrap();
        assert_eq!(got.map(|r| r.fingerprint), Some("red/new//".to_string()));
    }

    #[test]
    fn a_wait_ends_when_the_coach_stops_being_ours_mid_wait() {
        let slot = Mutex::new(None);
        let arrived = Condvar::new();
        let started = Instant::now();
        let flips = std::cell::Cell::new(0);
        let got = wait_for_recommendation(
            &slot,
            &arrived,
            "red///",
            Instant::now() + Duration::from_secs(10),
            &|| {
                flips.set(flips.get() + 1);
                flips.get() < 3
            },
        );
        assert!(got.is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}

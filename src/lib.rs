//! LT AI Coach Drafter — lets LT AI Coach draft for the user in game.
//!
//! The game already has "Delegate to Head Coach" on the draft screen. While it
//! runs, the game asks the stable draft hook (`decide_ban` / `decide_pick`) for
//! every decision of the delegated side, and honours the answer. This mod
//! answers from LT AI Coach's recommendation, and puts a look-alike button over
//! the game's own so the player sees "Delegate ban/pick to LT Coach". The click
//! falls through to the game's button, which starts the game's own delegation.
//!
//! Evidence for each of those statements is in `LT_AiCoach/docs/draft-delegation-plan.md`
//! (probe `lt_drafter_probe`, game 0.6.2).
//!
//! Safety rule: answer only when sure. Every check that fails makes the hook
//! return `None`, and the game's own Head Coach then decides that turn as if this
//! mod were not installed. The hook never blocks on I/O: a receiver thread owns
//! the socket, and the hook waits on a `Condvar`, only after every other check
//! has passed, for a bounded time. `score_*` always passes: the opponent's AI
//! consults them on the live board.
//!
//! Last updated: 2026-10-06. Targets the stable mod API; no rebuild per game update.

pub mod logic;
pub mod protocol;
pub mod strings;
pub mod swap;

use std::collections::BTreeSet;
use std::io::Write;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use mod_api_stable::{
    declare_stable_mod, LogLevel, StableClient, StableDraftContext, StableDraftDecision,
    StableDraftHook, StableExtension, StableHost, StableMod,
};

use logic::{Action, Decline, HookBoard, LiveBoard, Side};
use protocol::{Packet, Recommendation, SwapRecommendation};

const MOD_ID: &str = "lt_ai_coach_drafter";
const LISTEN_ADDRESS: &str = "127.0.0.1:32146";
const GRID: &str = "main.champions.contents";
const DELEGATE_BUTTON: &str = "main.header.delegate_btn";
const CANCEL_BUTTON: &str = "main.coach_delegation_indicator.cancel";
const OVERLAY: &str = "main.coach_delegation_indicator";
const OVERLAY_TEXT: &str = "main.coach_delegation_indicator.text";
/// "Finish Ban/Pick Now" (game 0.6.3+): finishes the draft without asking the draft hooks.
const INSTANT_BUTTON: &str = "main.coach_delegation_indicator.instant";
const INSTANT_WARNING: &str = "main.coach_delegation_indicator.lt_coach_instant_warning";
const TIMER_TEXT: &str = "main.timer_area.timer_text";
const COVER: &str = "main.header.lt_coach_delegate_cover";
const WARN_BUTTON: &str = "main.header.lt_coach_delegate_warn";
const WARNING: &str = "main.lt_coach_warning";
const WARNING_OK: &str = "main.lt_coach_warning.panel.ok";
const SWAP_ROOT: &str = "main.swap";
const SWAP_COACH_BUTTON: &str = "main.swap.bottom.coach";
const SWAP_COVER: &str = "main.lt_coach_swap_cover";
const POLL_INTERVAL_MICROS: u64 = 250_000;
const LOG_LIMIT: u64 = 512 * 1024;
/// The Coach sends a heartbeat every second; this many milliseconds without one
/// and it is treated as not running.
const HEARTBEAT_TIMEOUT_MS: u64 = 3_000;

// ------------------------------------------------------------------- logging

static LOG_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
static LOG_LOCK: Mutex<()> = Mutex::new(());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn log_path() -> Option<&'static PathBuf> {
    LOG_PATH
        .get_or_init(|| {
            let dir = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?)
                .join("LTTools")
                .join(MOD_ID);
            std::fs::create_dir_all(&dir).ok()?;
            Some(dir.join("drafter.log"))
        })
        .as_ref()
}

fn log(message: &str) {
    let Some(path) = log_path() else { return };
    let _guard = lock(&LOG_LOCK);
    if std::fs::metadata(path).map_or(false, |m| m.len() > LOG_LIMIT) {
        let _ = std::fs::rename(path, path.with_file_name("drafter.previous.log"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}\t{message}", now_ms());
    }
}

/// Lines produced on hook threads, written by the client thread. Hook threads
/// only `try_lock`, so a busy queue drops a line rather than stalling a worker.
static HOOK_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
const HOOK_LINE_CAP: usize = 200;

fn hook_log(line: String) {
    if let Ok(mut lines) = HOOK_LINES.try_lock() {
        if lines.len() < HOOK_LINE_CAP {
            lines.push(line);
        }
    }
}

// ----------------------------------------------------- the Coach's messages

static HEARTBEAT_AT_MS: AtomicU64 = AtomicU64::new(0);
static COACH_ENABLED: AtomicBool = AtomicBool::new(false);
/// The side the Coach says it is ready to recommend for: 0 none, 1 blue, 2 red.
static COACH_SIDE: AtomicU8 = AtomicU8::new(0);
static LANGUAGE: Mutex<String> = Mutex::new(String::new());
static RECOMMENDATION: Mutex<Option<Recommendation>> = Mutex::new(None);
static RECOMMENDATION_ARRIVED: Condvar = Condvar::new();
/// The user switched on swap-stage delegation in the Coach (heartbeat flag).
static COACH_SWAP: AtomicBool = AtomicBool::new(false);
static SWAP_RECOMMENDATION: Mutex<Option<SwapRecommendation>> = Mutex::new(None);
/// The swap hook writes LT Coach's order (the Coach's swap switch is the user's consent);
/// a `swap_dry_run.enabled` marker present when the draft screen was entered makes it
/// log what it would write instead (for checking a newly added game build).
static SWAP_LIVE: AtomicBool = AtomicBool::new(false);
static NATIVE_ATTEMPTED: AtomicBool = AtomicBool::new(false);

/// What the Coach app is doing, as far as its heartbeat says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoachState {
    /// No recent heartbeat: the app is not running.
    Absent,
    /// Running, with LT Coach drafting switched off in its settings.
    Off,
    /// Switched on, but it cannot recommend right now (no live user side, no data).
    NotReady,
    /// Switched on and ready to recommend for this side.
    Ready(Side),
}

impl CoachState {
    fn describe(self) -> &'static str {
        match self {
            CoachState::Absent => "not running",
            CoachState::Off => "running with LT Coach drafting switched off",
            CoachState::NotReady => "switched on but unable to recommend yet",
            CoachState::Ready(_) => "ready to recommend",
        }
    }
}

fn coach_state() -> CoachState {
    let fresh = now_ms().saturating_sub(HEARTBEAT_AT_MS.load(Ordering::Relaxed)) < HEARTBEAT_TIMEOUT_MS;
    if !fresh {
        return CoachState::Absent;
    }
    if !COACH_ENABLED.load(Ordering::Relaxed) {
        return CoachState::Off;
    }
    match COACH_SIDE.load(Ordering::Relaxed) {
        1 => CoachState::Ready(Side::Blue),
        2 => CoachState::Ready(Side::Red),
        _ => CoachState::NotReady,
    }
}

/// The side the Coach is ready to recommend for, or `None` when it is not
/// running, has drafting switched off, has gone quiet, or has nothing to
/// recommend (no live user side, no data).
fn coach_side() -> Option<Side> {
    match coach_state() {
        CoachState::Ready(side) => Some(side),
        _ => None,
    }
}

fn receive_forever() {
    loop {
        let socket = match UdpSocket::bind(LISTEN_ADDRESS) {
            Ok(socket) => socket,
            Err(error) => {
                log(&format!("cannot listen on {LISTEN_ADDRESS}: {error} — retrying in 5 s"));
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
        };
        log(&format!("listening on {LISTEN_ADDRESS}"));
        let mut buffer = [0u8; 8192];
        loop {
            let Ok((length, _)) = socket.recv_from(&mut buffer) else {
                break;
            };
            let Ok(text) = std::str::from_utf8(&buffer[..length]) else { continue };
            match protocol::parse(text) {
                Some(Packet::Heartbeat { enabled, language, side, swap, .. }) => {
                    COACH_ENABLED.store(enabled, Ordering::Relaxed);
                    COACH_SWAP.store(swap, Ordering::Relaxed);
                    COACH_SIDE.store(
                        match side.as_deref().and_then(Side::from_word) {
                            Some(Side::Blue) => 1,
                            Some(Side::Red) => 2,
                            None => 0,
                        },
                        Ordering::Relaxed,
                    );
                    HEARTBEAT_AT_MS.store(now_ms(), Ordering::Relaxed);
                    let mut current = lock(&LANGUAGE);
                    if *current != language {
                        *current = language;
                    }
                }
                Some(Packet::Recommendation(recommendation)) => {
                    *lock(&RECOMMENDATION) = Some(recommendation);
                    RECOMMENDATION_ARRIVED.notify_all();
                }
                Some(Packet::Swap(recommendation)) => {
                    *lock(&SWAP_RECOMMENDATION) = Some(recommendation);
                }
                None => {}
            }
        }
    }
}

// ------------------------------------------------------------- what is on screen

/// What the client thread last read. Hook threads take an `Arc` clone with
/// `try_lock` and never hold the mutex while working.
struct Live {
    board: LiveBoard,
    /// 1 blue, 2 red, 3 both: the sides whose `in_turn` marker is lit.
    turn: u8,
    overlay_visible: bool,
    timer_seconds: Option<u32>,
    /// The swap stage's panel is showing.
    swap_visible: bool,
    /// Each side's picks in draft order (the order badge on the grid tiles).
    blue_order: Vec<String>,
    red_order: Vec<String>,
}

static LIVE: Mutex<Option<Arc<Live>>> = Mutex::new(None);
/// Set the instant the game's delegate button is clicked, closing the gap before
/// the next poll sees the overlay; cleared by Cancel and when the overlay hides.
static DELEGATE_CLICKED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------- hook

struct DrafterHook;

/// Last time a near-miss was logged, so a timing race can be told apart from a
/// genuine mismatch without a line per background call.
static LAST_NEAR_MISS_MS: AtomicU64 = AtomicU64::new(0);
const NEAR_MISS_EVERY_MS: u64 = 10_000;
/// Same idea for pool-mismatch declines (see `answer`).
static LAST_POOL_DECLINE_MS: AtomicU64 = AtomicU64::new(0);
const POOL_DECLINE_EVERY_MS: u64 = 5_000;

impl DrafterHook {
    fn answer(action: Action, ctx: &StableDraftContext<'_>) -> Option<usize> {
        let live = LIVE.try_lock().ok()?.clone()?;
        if !(live.overlay_visible || DELEGATE_CLICKED.load(Ordering::Relaxed)) {
            return None;
        }
        let word = match action {
            Action::Ban => "ban",
            Action::Pick => "pick",
        };

        // The hook's board as names, plus the name -> id table for the answer.
        let name = |id: &usize| ctx.champion_name(*id).map(str::to_string);
        let names = |ids: &[usize]| -> Option<Vec<String>> { ids.iter().map(name).collect() };
        let (ally_bans, enemy_bans, ally_picks, enemy_picks) = (
            names(ctx.ally_bans())?,
            names(ctx.enemy_bans())?,
            names(ctx.ally_picks())?,
            names(ctx.enemy_picks())?,
        );
        let mut ids_by_name = std::collections::BTreeMap::new();
        for id in ctx.available_champions() {
            if let Some(champion) = name(id) {
                ids_by_name.insert(champion, *id);
            }
        }
        let hook = HookBoard {
            ally_bans,
            enemy_bans,
            ally_picks,
            enemy_picks,
            available: ids_by_name.keys().cloned().collect::<BTreeSet<String>>(),
        };

        // Which side is acting comes from the screen. When it cannot be named, try
        // both readings: a board that matches under one is the live draft, and
        // worth a log line, whereas one that matches under neither is ordinary
        // background traffic.
        let Some(acting) = Side::from_turn_bits(live.turn) else {
            let live_draft = [Side::Blue, Side::Red]
                .into_iter()
                .any(|side| logic::check_board(&live.board, &hook, side).is_ok());
            if live_draft {
                hook_log(format!(
                    "declined {word}: the board matches the screen but the screen does not name a single acting side (turn bits {})",
                    live.turn
                ));
            }
            return None;
        };

        match logic::check_board(&live.board, &hook, acting) {
            Ok(()) => {}
            Err(Decline::BoardMismatch) => {
                // The ordinary case: background simulations call this hook
                // constantly. Only a near-miss while the overlay is up is worth a
                // line — the screen snapshot lags by up to a poll, so a call that
                // is one action off may simply have been early.
                let (bans_off, picks_off) = logic::count_gap(&live.board, &hook);
                if live.overlay_visible && bans_off + picks_off <= 1 {
                    let now = now_ms();
                    if now.saturating_sub(LAST_NEAR_MISS_MS.load(Ordering::Relaxed)) >= NEAR_MISS_EVERY_MS {
                        LAST_NEAR_MISS_MS.store(now, Ordering::Relaxed);
                        hook_log(format!(
                            "near miss on {word}: the hook's board is {bans_off} ban(s) and {picks_off} pick(s) from the screen's (a timing race, or a background simulation)"
                        ));
                    }
                }
                return None;
            }
            Err(Decline::PoolMismatch { extra_in_hook, missing_from_hook }) => {
                // An empty board matches every background simulation that has just
                // started (they all begin empty, with a smaller pool), so a mismatch
                // there is ordinary traffic, not a sign of trouble. With at least one
                // action on the board a pool mismatch is real information — it is
                // how a Fearless blocked-tile reading that is wrong would show up —
                // but even then at most one line per interval.
                let actions = live.board.bans.len() + live.board.blue_picks.len() + live.board.red_picks.len();
                let now = now_ms();
                if actions > 0
                    && now.saturating_sub(LAST_POOL_DECLINE_MS.load(Ordering::Relaxed)) >= POOL_DECLINE_EVERY_MS
                {
                    LAST_POOL_DECLINE_MS.store(now, Ordering::Relaxed);
                    hook_log(format!(
                        "declined {word}: the board matches the screen but the candidate pool does not (hook has {extra_in_hook} the screen excludes, lacks {missing_from_hook} the screen allows; {actions} action(s) on the board)"
                    ));
                }
                return None;
            }
        }

        // From here on the call is the live draft. Never make the opponent's AI
        // wait: a decision for the side the Coach is not recommending for is not
        // ours, whatever has or has not arrived.
        if !logic::is_ours(coach_side(), acting) {
            hook_log(format!(
                "declined {word}: not ours (LT Coach is ready for {:?}, {} is acting)",
                coach_side().map(Side::word),
                acting.word()
            ));
            return None;
        }

        let fingerprint = hook.fingerprint(acting);
        let started = Instant::now();
        let deadline = started + logic::wait_budget(live.timer_seconds);
        let recommendation = logic::wait_for_recommendation(
            &RECOMMENDATION,
            &RECOMMENDATION_ARRIVED,
            &fingerprint,
            deadline,
            &|| logic::is_ours(coach_side(), acting),
        );
        let waited = started.elapsed().as_millis();
        let Some(recommendation) = recommendation else {
            hook_log(format!("declined {word}: no recommendation for this board within {waited} ms (fingerprint {fingerprint})"));
            return None;
        };
        let ranked = match action {
            Action::Ban => &recommendation.bans,
            Action::Pick => &recommendation.picks,
        };
        let Some(choice) = logic::choose(ranked, &hook.available) else {
            hook_log(format!("declined {word}: none of the Coach's {} ranked champions is offered by the game", ranked.len()));
            return None;
        };
        let id = *ids_by_name.get(choice)?;
        hook_log(format!(
            "answered {word} as {}: {choice} (id {id}, rank {} of {}, waited {waited} ms)",
            acting.word(),
            ranked.iter().position(|c| c == choice).map_or(0, |p| p + 1),
            ranked.len()
        ));
        Some(id)
    }
}

impl StableDraftHook for DrafterHook {
    fn id(&self) -> String {
        MOD_ID.to_string()
    }
    // The opponent's AI consults these on the live board; leave them alone.
    fn score_ban(&self, _ctx: &StableDraftContext<'_>, _c: usize, _b: f32) -> StableDraftDecision {
        StableDraftDecision::Pass
    }
    fn score_pick(&self, _ctx: &StableDraftContext<'_>, _c: usize, _b: f32) -> StableDraftDecision {
        StableDraftDecision::Pass
    }
    fn decide_ban(&self, ctx: &StableDraftContext<'_>) -> Option<usize> {
        Self::answer(Action::Ban, ctx)
    }
    fn decide_pick(&self, ctx: &StableDraftContext<'_>) -> Option<usize> {
        Self::answer(Action::Pick, ctx)
    }
}

// --------------------------------------------------------------- screen reads

fn visible(ctx: &StableClient<'_>, path: &str) -> bool {
    ctx.ui_visible(path) == Some(true)
}

fn on_draft_screen(ctx: &StableClient<'_>) -> bool {
    ctx.ui_exists(GRID) && ctx.ui_exists("main.bottom.blue_side.bans")
}

fn read_board(ctx: &StableClient<'_>) -> LiveBoard {
    let mut board = LiveBoard::default();
    for champion in ctx.ui_child_names(GRID) {
        let tile = format!("{GRID}.{champion}");
        if visible(ctx, &format!("{tile}.ban")) {
            board.bans.insert(champion.clone());
        }
        if visible(ctx, &format!("{tile}.blue")) {
            board.blue_picks.insert(champion.clone());
        }
        if visible(ctx, &format!("{tile}.red")) {
            board.red_picks.insert(champion.clone());
        }
        if visible(ctx, &format!("{tile}.fearless_x")) || visible(ctx, &format!("{tile}.fearless_icon")) {
            board.blocked.insert(champion.clone());
        }
        board.grid.insert(champion);
    }
    board
}

/// One side's picks in draft order: the grid tile's order badge (1..5), unreadable
/// badges last, exactly as the bridge reads them.
fn read_pick_order(ctx: &StableClient<'_>, side: &str) -> Vec<String> {
    let mut picks: Vec<(u8, String)> = Vec::new();
    for champion in ctx.ui_child_names(GRID) {
        let tile = format!("{GRID}.{champion}");
        if visible(ctx, &format!("{tile}.{side}")) {
            let order = ctx
                .ui_text(&format!("{tile}.{side}.text"))
                .and_then(|t| t.trim().parse::<u8>().ok())
                .unwrap_or(u8::MAX);
            picks.push((order, champion));
        }
    }
    picks.sort();
    picks.into_iter().map(|(_, champion)| champion).take(5).collect()
}

/// The sides whose `in_turn` outline is lit (1 blue, 2 red, 3 both).
fn read_turn(ctx: &StableClient<'_>) -> u8 {
    let mut bits = 0u8;
    for (bit, side) in [(1u8, "blue"), (2u8, "red")] {
        for index in 0..5 {
            let ban = format!("main.bottom.{side}_side.bans.ban_slot_{index}");
            if (visible(ctx, &ban) && visible(ctx, &format!("{ban}.in_turn")))
                || visible(ctx, &format!("main.{side}_picks.pick_slot_{index}.in_turn"))
            {
                bits |= bit;
            }
        }
    }
    bits
}

// --------------------------------------------------------------- the session

#[derive(Default)]
struct Session {
    elapsed: u64,
    in_draft: bool,
    cover_attempts: u32,
    warn_attempts: u32,
    panel_attempts: u32,
    warn_registered: bool,
    panel_registered: bool,
    seen_overlay: bool,
    original_overlay_text: Option<String>,
    applied_language: Option<String>,
    /// The game's own language, recognised from its localised delegate-button text.
    game_language: Option<&'static str>,
    applied_ready: Option<bool>,
    last_state: Option<CoachState>,
    swap_cover_attempts: u32,
    instant_warning_attempts: u32,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// A UI rectangle `(x, y, width, height)` in the layout's own units.
type Rect = (f32, f32, f32, f32);

/// Where the game's buttons sit in this build (measured in game 0.6.2). Used only when
/// the live rectangle is unavailable, so a covered button still lines up if a game update
/// or another mod moves or resizes the real one.
const DELEGATE_RECT: Rect = (10.0, 5.0, 290.0, 40.0);
const SWAP_COACH_RECT: Rect = (310.0, 903.0, 645.0, 67.0);

/// The node's laid-out rectangle, or `fallback` when it has none yet (not laid out,
/// hidden, or missing) or the numbers are not sane.
fn rect_or(ctx: &StableClient<'_>, path: &str, fallback: Rect) -> Rect {
    match ctx.ui_node_rect(path) {
        Some((x, y, w, h))
            if w >= 8.0 && h >= 8.0 && w <= 4000.0 && h <= 2000.0 && x >= -100.0 && y >= -100.0
                && x.is_finite() && y.is_finite() =>
        {
            (x, y, w, h)
        }
        _ => fallback,
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Looks like the game's delegate button (same slot, fill and text colour) but is
/// built from non-interactive `color` + `label` nodes with `ignore_event`, so a
/// click falls through to the game's button underneath. (A `color_icon_button`
/// with `ignore_event` swallows the click — probe v0.3.) Shown while the Coach is
/// ready to draft.
fn cover_source(text: &str, rect: Rect) -> String {
    format!(
        r##"lt_coach_delegate_cover:color {{
  x: {}px;
  y: {}px;
  width: {}px;
  height: {}px;
  color: #d9f4efff;
  rounding: Uniform {{ rounding: 8; }}
  ignore_event: true;

  #text:label {{
    @"asset/base/style/main#bold_label";
    width: 100%;
    height: 100%;
    ignore_event: true;
    text: "{}";
    color: #0f5b4dff;
    align_x: Center;
    align_y: Center;
    size: 14;
  }}
}}"##,
        rect.0,
        rect.1,
        rect.2,
        rect.3,
        escape(text)
    )
}

/// The same look, but a real button: it takes the click itself, so the game's
/// own delegation does not start. Shown while the Coach is running with LT Coach
/// drafting switched off; clicking it explains how to switch it on.
fn warn_button_source(text: &str, rect: Rect) -> String {
    format!(
        r##"lt_coach_delegate_warn:color_icon_button {{
  @"asset/base/style/main#secondary_button";
  x: {}px;
  y: {}px;
  width: {}px;
  height: {}px;
  text: {{
    font: "asset/base/font/set/bold";
    text: "{}";
    align_x: Center;
    align_y: Center;
    size: 14;
  }}
}}"##,
        rect.0,
        rect.1,
        rect.2,
        rect.3,
        escape(text)
    )
}

/// The explanation, hidden until the warn button is clicked.
fn warning_source(texts: &strings::Texts) -> String {
    let line = |name: &str, y: u32, size: u32, bold: bool, text: &str| {
        format!(
            r##"    #{name}:label {{
      @"asset/base/style/main#{}";
      x: 0px;
      y: {y}px;
      width: 100%;
      height: 34px;
      ignore_event: true;
      text: "{}";
      color: #e8e8e8ff;
      align_x: Center;
      align_y: Center;
      size: {size};
    }}
"##,
            if bold { "bold_label" } else { "label" },
            escape(text)
        )
    };
    format!(
        r##"lt_coach_warning:color {{
  width: 100%;
  height: 100%;
  color: #000000b8;
  visible: false;

  #panel:color {{
    width: 900px;
    height: 300px;
    anchor_x: 0.5;
    pivot_x: 0.5;
    anchor_y: 0.5;
    pivot_y: 0.5;
    color: #161721ff;
    stroke: 1;
    back_color: #161721ff;
    rounding: Uniform {{ rounding: 12; }}

{}{}{}{}
    #ok:color_icon_button {{
      @"asset/base/style/main#primary_button";
      width: 200px;
      height: 44px;
      anchor_x: 0.5;
      pivot_x: 0.5;
      anchor_y: 1;
      pivot_y: 1;
      y: -22px;
      text: {{
        font: "asset/base/font/set/bold";
        text: "{}";
        align_x: Center;
        align_y: Center;
        size: 18;
      }}
    }}
  }}
}}"##,
        line("title", 24, 26, true, texts.warn_title),
        line("line1", 88, 18, false, texts.warn_line1),
        line("line2", 122, 18, false, texts.warn_line2),
        line("line3", 156, 18, false, texts.warn_line3),
        escape(texts.ok)
    )
}

fn click_listener(ctx: &mut StableClient<'_>, path: &str, then: impl Fn(&mut StableClient<'_>) + Send + Sync + 'static) -> bool {
    ctx.ui_register_path_events(path, move |c| {
        if c.ui_current_event().is_some_and(|e| e.kind == Some(mod_api_stable::UiEventKindV1::Click)) {
            then(c);
        }
    })
}

fn register_click_listeners(ctx: &mut StableClient<'_>) {
    let started = click_listener(ctx, DELEGATE_BUTTON, |_| DELEGATE_CLICKED.store(true, Ordering::Relaxed));
    let cancelled = click_listener(ctx, CANCEL_BUTTON, |_| DELEGATE_CLICKED.store(false, Ordering::Relaxed));
    log(&format!("click listeners: delegate={started} cancel={cancelled}"));
}

fn on_enter(ctx: &mut StableClient<'_>, session: &mut Session) {
    session.in_draft = true;
    session.cover_attempts = 0;
    session.warn_attempts = 0;
    session.panel_attempts = 0;
    session.swap_cover_attempts = 0;
    session.instant_warning_attempts = 0;
    session.warn_registered = false;
    session.panel_registered = false;
    SWAP_LIVE.store(!marker("swap_dry_run.enabled"), Ordering::Relaxed);
    session.seen_overlay = false;
    session.applied_language = None;
    session.applied_ready = None;
    // The game's language decides what the look-alike buttons say. Asked of the game
    // itself: it resolves its own "Delegate ban/pick to Head Coach" text for the language
    // in use, and that text identifies the language (see strings.rs).
    let own_text = ctx.i18n("#asset/base/text/ui?banpick.delegate_to_coach");
    session.game_language = own_text.as_deref().and_then(strings::detect_game_language);
    log(&format!(
        "game language: {} (the game's own delegate text: {:?})",
        session.game_language.unwrap_or("not recognised; falling back to the Coach's language"),
        own_text
    ));
    session.original_overlay_text = ctx.ui_text(OVERLAY_TEXT);
    DELEGATE_CLICKED.store(false, Ordering::Relaxed);
    *lock(&RECOMMENDATION) = None;
    log("entered the draft screen");
    register_click_listeners(ctx);
}

fn on_leave(session: &mut Session) {
    session.in_draft = false;
    *lock(&LIVE) = None;
    *lock(&RECOMMENDATION) = None;
    DELEGATE_CLICKED.store(false, Ordering::Relaxed);
    log("left the draft screen");
}

/// Spawns `source` under `parent` when the node is missing (at most three tries
/// per draft screen). Returns whether the node exists afterwards.
fn ensure_node(ctx: &mut StableClient<'_>, parent: &str, path: &str, source: &str, attempts: &mut u32) -> bool {
    if !ctx.ui_exists(path) && *attempts < 3 {
        *attempts += 1;
        let ok = ctx.ui_spawn_source(parent, source);
        log(&format!("spawn {path}: attempt {attempts} -> {ok}"));
    }
    ctx.ui_exists(path)
}

fn set_visible_if_needed(ctx: &mut StableClient<'_>, path: &str, show: bool) {
    if ctx.ui_exists(path) && ctx.ui_visible(path) != Some(show) {
        ctx.ui_set_visible(path, show);
    }
}

/// Keeps the buttons, the warning and the overlay text in step with the Coach.
///
/// * Coach ready: the click-through cover is shown.
/// * Coach running but LT Coach drafting off: the clickable warn button is shown.
/// * Coach absent, or enabled but unable to recommend: neither — the game's own
///   button is untouched, so delegation to the game's Head Coach still works.
fn sync_ui(ctx: &mut StableClient<'_>, session: &mut Session, native_visible: bool) {
    let state = coach_state();
    let ready = matches!(state, CoachState::Ready(_));
    if session.last_state != Some(state) {
        session.last_state = Some(state);
        log(&format!("LT Coach is now: {}", state.describe()));
    }

    let coach_language = lock(&LANGUAGE).clone();
    let texts = strings::compose(session.game_language, &coach_language);
    // What the texts depend on: the game's language and the Coach's.
    let language = format!("{}|{coach_language}", session.game_language.unwrap_or("?"));
    // A text change means rebuilding: button text cannot be rewritten in place.
    if session.applied_language.as_deref().is_some_and(|applied| applied != language) {
        for path in [COVER, WARN_BUTTON, WARNING, SWAP_COVER, INSTANT_WARNING] {
            if ctx.ui_exists(path) {
                ctx.ui_remove_node(path);
            }
        }
        session.cover_attempts = 0;
        session.warn_attempts = 0;
        session.panel_attempts = 0;
        session.swap_cover_attempts = 0;
        session.instant_warning_attempts = 0;
        session.warn_registered = false;
        session.panel_registered = false;
    }
    session.applied_language = Some(language);

    let delegate_rect = rect_or(ctx, DELEGATE_BUTTON, DELEGATE_RECT);
    ensure_node(ctx, "main.header", COVER, &cover_source(texts.cover, delegate_rect), &mut session.cover_attempts);
    if ensure_node(ctx, "main.header", WARN_BUTTON, &warn_button_source(texts.cover, delegate_rect), &mut session.warn_attempts)
        && !session.warn_registered
    {
        session.warn_registered = true;
        let ok = click_listener(ctx, WARN_BUTTON, |c| {
            c.ui_set_visible(WARNING, true);
        });
        log(&format!("warn button listener: {ok}"));
    }
    if ensure_node(ctx, "main", WARNING, &warning_source(&texts), &mut session.panel_attempts)
        && !session.panel_registered
    {
        session.panel_registered = true;
        let ok = click_listener(ctx, WARNING_OK, |c| {
            c.ui_set_visible(WARNING, false);
        });
        log(&format!("warning ok listener: {ok}"));
    }

    set_visible_if_needed(ctx, COVER, native_visible && ready);
    // The warn button takes the click away from the game's own delegate button, so it is
    // only shown when the explanation it opens actually exists and its click listener is
    // registered. If either failed (a different game build, a changed UI), the game's own
    // button stays usable instead of being silently blocked.
    let warning_works = session.warn_registered && session.panel_registered && ctx.ui_exists(WARNING);
    set_visible_if_needed(
        ctx,
        WARN_BUTTON,
        native_visible && state == CoachState::Off && warning_works,
    );

    if session.applied_ready != Some(ready) {
        // Ours while the Coach drafts; the game's own words otherwise, so a user
        // delegating without the Coach is not told it is the Coach.
        let text = if ready {
            Some(texts.overlay.to_string())
        } else {
            session.original_overlay_text.clone()
        };
        if let Some(text) = text {
            ctx.ui_set_text(OVERLAY_TEXT, &text);
        }
        session.applied_ready = Some(ready);
    }

    // The game's "Finish Ban/Pick Now" (0.6.3+) completes the draft without asking the
    // draft hooks, so LT Coach is skipped from then on. Said under the buttons while LT
    // Coach drafts and the button is offered; builds without the button never spawn it.
    let instant_offered = ready && visible(ctx, INSTANT_BUTTON);
    if instant_offered {
        ensure_node(
            ctx,
            OVERLAY,
            INSTANT_WARNING,
            &instant_warning_source(texts.instant_warning),
            &mut session.instant_warning_attempts,
        );
    }
    set_visible_if_needed(ctx, INSTANT_WARNING, instant_offered);
}

/// A line under the delegation overlay's buttons (which sit 30-70 px below its centre).
fn instant_warning_source(text: &str) -> String {
    format!(
        r##"lt_coach_instant_warning:label {{
  @"asset/base/style/main#bold_label";
  width: 100%;
  height: 32px;
  anchor_y: 0.5;
  pivot_y: 0;
  y: 86px;
  ignore_event: true;
  text: "{}";
  color: #ffc857ff;
  align_x: Center;
  align_y: Center;
  size: 20;
}}"##,
        escape(text)
    )
}

/// The look-alike for the swap stage's Coach button (same slot, tertiary-button look),
/// click-through so the game's own button underneath still receives the press.
fn swap_cover_source(text: &str, rect: Rect) -> String {
    format!(
        r##"lt_coach_swap_cover:color {{
  x: {}px;
  y: {}px;
  width: {}px;
  height: {}px;
  color: #6f7788ff;
  stroke: 1;
  back_color: #20232dff;
  rounding: Uniform {{ rounding: 8; }}
  ignore_event: true;

  #text:label {{
    @"asset/base/style/main#bold_label";
    width: 100%;
    height: 100%;
    ignore_event: true;
    text: "{}";
    color: #d7dbe4ff;
    align_x: Center;
    align_y: Center;
    size: 24;
  }}
}}"##,
        rect.0,
        rect.1,
        rect.2,
        rect.3,
        escape(text)
    )
}

/// Shows "Swap with LT Coach" over the game's swap Coach button, only while the native
/// hook is installed, live, enabled in the Coach and the Coach is ready for a side.
fn sync_swap_ui(ctx: &mut StableClient<'_>, session: &mut Session, swap_visible: bool) {
    let show = swap_visible
        && swap::is_installed()
        && SWAP_LIVE.load(Ordering::Relaxed)
        && COACH_SWAP.load(Ordering::Relaxed)
        && coach_side().is_some()
        && visible(ctx, SWAP_COACH_BUTTON);
    if show {
        let coach_language = lock(&LANGUAGE).clone();
        let texts = strings::compose(session.game_language, &coach_language);
        let rect = rect_or(ctx, SWAP_COACH_BUTTON, SWAP_COACH_RECT);
        ensure_node(ctx, "main", SWAP_COVER, &swap_cover_source(texts.swap_cover, rect), &mut session.swap_cover_attempts);
    }
    set_visible_if_needed(ctx, SWAP_COVER, show);
}

fn poll(ctx: &mut StableClient<'_>, session: &mut Session) {
    let board = read_board(ctx);
    let overlay_visible = visible(ctx, OVERLAY);
    if overlay_visible {
        session.seen_overlay = true;
    } else if session.seen_overlay {
        // Delegation ended (the picks are done, or the user cancelled).
        session.seen_overlay = false;
        DELEGATE_CLICKED.store(false, Ordering::Relaxed);
    }
    let timer_seconds = ctx.ui_text(TIMER_TEXT).and_then(|t| logic::parse_timer(&t));
    let swap_visible = visible(ctx, SWAP_ROOT);
    // The pick order is only needed once the swap stage is up (or about to be): reading
    // it costs a few calls per picked tile, so it is skipped while the draft is early.
    let (blue_order, red_order) = if swap_visible || (board.blue_picks.len() == 5 && board.red_picks.len() == 5) {
        (read_pick_order(ctx, "blue"), read_pick_order(ctx, "red"))
    } else {
        (Vec::new(), Vec::new())
    };
    *lock(&LIVE) = Some(Arc::new(Live {
        board,
        turn: read_turn(ctx),
        overlay_visible,
        timer_seconds,
        swap_visible,
        blue_order,
        red_order,
    }));
    sync_ui(ctx, session, visible(ctx, DELEGATE_BUTTON));
    sync_swap_ui(ctx, session, swap_visible);
}

fn log_dir() -> Option<PathBuf> {
    log_path().and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// True when the named marker file exists beside the log.
fn marker(name: &str) -> bool {
    log_dir().is_some_and(|dir| dir.join(name).exists())
}

/// Installs the swap hook, once, on the title screen, if the user wants it.
///
/// "Wants it" is the Coach's heartbeat swap flag, or a `swap_native.enabled` marker
/// file. The executable is only hashed once somebody wants the hook, and nothing is
/// ever patched after the title screen: a later request waits for the next launch.
fn native_tick(ctx: &mut StableClient<'_>) {
    if swap::is_installed() || NATIVE_ATTEMPTED.load(Ordering::Relaxed) {
        return;
    }
    let wanted = || {
        marker("swap_native.enabled")
            || (COACH_SWAP.load(Ordering::Relaxed) && coach_state() != CoachState::Absent)
    };
    match ctx.scene_kind() {
        Some(mod_api_stable::SceneKindV1::Title) => {
            if !wanted() {
                return;
            }
            swap::begin_hashing();
            if swap::hash_result().is_none() {
                return; // still hashing; try again next frame
            }
            NATIVE_ATTEMPTED.store(true, Ordering::Relaxed);
            match swap::install(log_dir().as_deref()) {
                Ok(()) => log("swap: native hook installed"),
                Err(error) => log(&format!("swap: NOT INSTALLED: {error}")),
            }
        }
        Some(mod_api_stable::SceneKindV1::Loading) | None => {}
        Some(other) => {
            NATIVE_ATTEMPTED.store(true, Ordering::Relaxed);
            if wanted() {
                log(&format!(
                    "swap: NOT INSTALLED: the first frame seen was not the title screen ({other:?}); a restart is needed"
                ));
            }
        }
    }
}

static LAST_SWAP_NOTE: Mutex<std::collections::BTreeMap<&'static str, u64>> =
    Mutex::new(std::collections::BTreeMap::new());

/// A log line at most every few seconds per key (the dialogue site calls often).
fn note_swap(key: &'static str, message: String) {
    let now = now_ms();
    let mut last = lock(&LAST_SWAP_NOTE);
    if now.saturating_sub(last.get(key).copied().unwrap_or(0)) >= 5_000 {
        last.insert(key, now);
        log(&message);
    }
}

/// What the swap hook should write, if anything: LT Coach's order, when the Coach has
/// swap delegation on and ready for a side, the swap stage is on screen, and the Coach's
/// fingerprint of the user's picks (draft order) equals the screen's own. `None` leaves
/// the game's own answer untouched.
pub(crate) fn swap_target(site: usize, _current: &[u64], _this: usize) -> Option<([usize; 5], swap::Mode)> {
    let mode = if SWAP_LIVE.load(Ordering::Relaxed) { swap::Mode::Live } else { swap::Mode::DryRun };
    if !COACH_SWAP.load(Ordering::Relaxed) {
        return None;
    }
    let Some(side) = coach_side() else {
        note_swap("swap-no-side", "swap: the Coach is not ready to recommend for a side; left alone".to_string());
        return None;
    };
    let live = LIVE.try_lock().ok()?.clone()?;
    if !live.swap_visible {
        return None; // the dialogue builder also calls compute outside the swap stage
    }
    let picks = match side {
        Side::Blue => &live.blue_order,
        Side::Red => &live.red_order,
    };
    if picks.len() != 5 {
        note_swap("swap-picks", format!("swap: the screen shows {} {} picks, not 5; left alone", picks.len(), side.word()));
        return None;
    }
    let fingerprint = protocol::swap_fingerprint(side.word(), picks);
    let recommendation = lock(&SWAP_RECOMMENDATION).clone();
    match recommendation {
        Some(r) if r.fingerprint == fingerprint => Some((r.order, mode)),
        Some(r) => {
            note_swap(
                "swap-mismatch",
                format!(
                    "swap: {} call: the Coach's fingerprint {} differs from the screen's {fingerprint}; left alone",
                    ["apply", "dialogue"][site.min(1)],
                    r.fingerprint
                ),
            );
            None
        }
        None => {
            note_swap("swap-none", "swap: no swap recommendation from the Coach yet; left alone".to_string());
            None
        }
    }
}

struct DrafterExtension;

impl StableExtension for DrafterExtension {
    fn pre_update(&self, ctx: &mut StableClient<'_>, dt_micros: u64) {
        native_tick(ctx);
        let mut guard = lock(&SESSION);
        let session = guard.get_or_insert_with(Session::default);
        session.elapsed += dt_micros;
        if session.elapsed < POLL_INTERVAL_MICROS {
            return;
        }
        session.elapsed -= POLL_INTERVAL_MICROS;
        if session.elapsed >= POLL_INTERVAL_MICROS {
            session.elapsed = 0;
        }

        let on = on_draft_screen(ctx);
        if on && !session.in_draft {
            on_enter(ctx, session);
        } else if !on && session.in_draft {
            on_leave(session);
        }
        if on {
            poll(ctx, session);
        }
        let lines = std::mem::take(&mut *lock(&HOOK_LINES));
        for line in lines {
            log(&line);
        }
    }

    fn on_end(&self) {
        log("mod ended");
    }
}

fn init(host: &StableHost) -> StableMod {
    let version = host.game_version();
    host.log(
        LogLevel::Info,
        &format!(
            "{MOD_ID} loaded (game {}.{}.{}), log: {:?}",
            version.major,
            version.minor,
            version.patch,
            log_path()
        ),
    );
    log(&format!(
        "lt_ai_coach_drafter {} loaded against game {}.{}.{}",
        env!("CARGO_PKG_VERSION"),
        version.major,
        version.minor,
        version.patch
    ));
    std::thread::spawn(receive_forever);
    let mut declaration = StableMod::new(MOD_ID);
    declaration.set_extension(DrafterExtension);
    declaration.add_draft_score_hook(DrafterHook);
    declaration
}

declare_stable_mod!(init);

//! Wire format shared with the LT AI Coach app (`src-tauri/src/drafter.rs`).
//!
//! ```text
//! LTACD1|HB|<seq>|<enabled 0|1>|<language>|<side blue|red|->[|<swap 0|1>]
//! LTACD1|REC|<seq>|<fingerprint>|<ranked bans>|<ranked picks>
//! LTACD1|SWAP|<seq>|<swap fingerprint>|<o0,o1,o2,o3,o4>
//! ```
//!
//! The heartbeat's optional last field says whether the user also switched on swap-stage
//! delegation (absent = no, so an older Coach keeps working). The heartbeat's side is the
//! side the Coach is ready to recommend for, `-` when it is not. The fingerprint is `<side>/<sorted bans>/<sorted blue picks>/<sorted red
//! picks>`; see [`fingerprint`]. The shared fixture
//! `LT_AiCoach/src-tauri/tests/fixtures/drafter_fingerprints.tsv` is run against
//! it here and in the Coach so the two canonicalisers cannot drift.

const PROTOCOL: &str = "LTACD1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recommendation {
    pub seq: u64,
    pub fingerprint: String,
    pub bans: Vec<String>,
    pub picks: Vec<String>,
}

/// The Coach's arrangement for the swap stage. `order[row]` is the draft-order index
/// (0 = first drafted ... 4 = last drafted) of the champion that row should hold, rows
/// being Top, Jungle, Mid, Bot, Support — the same convention the game's own swap
/// Coach returns (the identity `[0,1,2,3,4]` is the arrangement before any swap).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapRecommendation {
    pub seq: u64,
    pub fingerprint: String,
    pub order: [usize; 5],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    Heartbeat {
        seq: u64,
        enabled: bool,
        language: String,
        /// `"blue"` or `"red"` when the Coach is ready to recommend for that side.
        side: Option<String>,
        /// The user also switched on swap-stage delegation.
        swap: bool,
    },
    Recommendation(Recommendation),
    Swap(SwapRecommendation),
}

/// `swap/<side>/<picks in draft order>`: the user's own champions, in the order they
/// were drafted (order matters here, unlike the ban/pick fingerprint).
pub fn swap_fingerprint<S: AsRef<str>>(side: &str, picks_in_draft_order: &[S]) -> String {
    let picks: Vec<&str> = picks_in_draft_order.iter().map(AsRef::as_ref).collect();
    format!("swap/{side}/{}", picks.join(","))
}

/// A permutation of `0..5`, or `None`.
pub fn parse_order(field: &str) -> Option<[usize; 5]> {
    let values: Vec<usize> = field
        .split(',')
        .map(|v| v.trim().parse().ok())
        .collect::<Option<Vec<_>>>()?;
    let order: [usize; 5] = values.try_into().ok()?;
    let mut seen = [false; 5];
    for &value in &order {
        if value >= 5 || std::mem::replace(&mut seen[value], true) {
            return None;
        }
    }
    Some(order)
}

fn canonical_list<S: AsRef<str>>(ids: &[S]) -> String {
    let mut sorted: Vec<&str> = ids.iter().map(AsRef::as_ref).collect();
    sorted.sort_unstable();
    sorted.join(",")
}

/// `<side>/<sorted bans>/<sorted blue picks>/<sorted red picks>`.
pub fn fingerprint<S: AsRef<str>>(side: &str, bans: &[S], blue_picks: &[S], red_picks: &[S]) -> String {
    format!(
        "{side}/{}/{}/{}",
        canonical_list(bans),
        canonical_list(blue_picks),
        canonical_list(red_picks)
    )
}

fn list(field: &str) -> Vec<String> {
    field
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parses one datagram. Anything that is not exactly a well-formed packet of
/// this protocol is `None`: the receiver ignores it rather than guessing.
pub fn parse(packet: &str) -> Option<Packet> {
    let mut fields = packet.trim().split('|');
    if fields.next()? != PROTOCOL {
        return None;
    }
    match fields.next()? {
        "HB" => {
            let seq = fields.next()?.parse().ok()?;
            let enabled = match fields.next()? {
                "1" => true,
                "0" => false,
                _ => return None,
            };
            let language = fields.next()?.to_string();
            let side = match fields.next()? {
                "-" => None,
                side @ ("blue" | "red") => Some(side.to_string()),
                _ => return None,
            };
            // Optional: older Coaches stop after the side.
            let swap = match fields.next() {
                None => false,
                Some("1") => true,
                Some("0") => false,
                Some(_) => return None,
            };
            fields
                .next()
                .is_none()
                .then_some(Packet::Heartbeat { seq, enabled, language, side, swap })
        }
        "SWAP" => {
            let seq = fields.next()?.parse().ok()?;
            let fingerprint = fields.next()?.to_string();
            let order = parse_order(fields.next()?)?;
            if !fingerprint.starts_with("swap/") || fields.next().is_some() {
                return None;
            }
            Some(Packet::Swap(SwapRecommendation { seq, fingerprint, order }))
        }
        "REC" => {
            let seq = fields.next()?.parse().ok()?;
            let fingerprint = fields.next()?.to_string();
            let bans = list(fields.next()?);
            let picks = list(fields.next()?);
            if fingerprint.is_empty() || fields.next().is_some() {
                return None;
            }
            Some(Packet::Recommendation(Recommendation { seq, fingerprint, bans, picks }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(field: &str) -> Vec<String> {
        list(field)
    }

    #[test]
    fn fingerprints_match_the_shared_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/drafter_fingerprints.tsv");
        let fixture = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read the shared fixture {path:?}: {e}"));
        let mut cases = 0;
        for line in fixture.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 5, "bad fixture line: {line:?}");
            let got = fingerprint(fields[0], &ids(fields[1]), &ids(fields[2]), &ids(fields[3]));
            assert_eq!(got, fields[4], "fixture line {line:?}");
            cases += 1;
        }
        assert!(cases >= 6, "the fixture lost cases");
    }

    #[test]
    fn parses_a_heartbeat() {
        assert_eq!(
            parse("LTACD1|HB|7|1|zh-Hant|red"),
            Some(Packet::Heartbeat {
                seq: 7,
                enabled: true,
                language: "zh-Hant".into(),
                side: Some("red".into()),
                swap: false,
            })
        );
        assert_eq!(
            parse("LTACD1|HB|8|0|en|-\n"),
            Some(Packet::Heartbeat { seq: 8, enabled: false, language: "en".into(), side: None, swap: false })
        );
        assert_eq!(
            parse("LTACD1|HB|9|1|en|blue|1"),
            Some(Packet::Heartbeat { seq: 9, enabled: true, language: "en".into(), side: Some("blue".into()), swap: true })
        );
        assert_eq!(
            parse("LTACD1|HB|9|1|en|blue|0"),
            Some(Packet::Heartbeat { seq: 9, enabled: true, language: "en".into(), side: Some("blue".into()), swap: false })
        );
    }

    #[test]
    fn parses_a_recommendation() {
        assert_eq!(
            parse("LTACD1|REC|3|red/a//|x,y|p"),
            Some(Packet::Recommendation(Recommendation {
                seq: 3,
                fingerprint: "red/a//".into(),
                bans: vec!["x".into(), "y".into()],
                picks: vec!["p".into()],
            }))
        );
        assert_eq!(
            parse("LTACD1|REC|4|red///||"),
            Some(Packet::Recommendation(Recommendation {
                seq: 4,
                fingerprint: "red///".into(),
                bans: vec![],
                picks: vec![],
            }))
        );
    }

    #[test]
    fn parses_a_swap_recommendation() {
        assert_eq!(
            parse("LTACD1|SWAP|5|swap/red/a,b,c,d,e|2,1,0,3,4"),
            Some(Packet::Swap(SwapRecommendation {
                seq: 5,
                fingerprint: "swap/red/a,b,c,d,e".into(),
                order: [2, 1, 0, 3, 4],
            }))
        );
    }

    #[test]
    fn swap_fingerprints_match_the_shared_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/drafter_swap_fingerprints.tsv");
        let fixture = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read the shared swap fixture {path:?}: {e}"));
        let mut cases = 0;
        for line in fixture.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 3, "bad fixture line: {line:?}");
            assert_eq!(swap_fingerprint(fields[0], &ids(fields[1])), fields[2], "fixture line {line:?}");
            cases += 1;
        }
        assert!(cases >= 3, "the swap fixture lost cases");
    }

    #[test]
    fn orders_must_be_permutations() {
        assert_eq!(parse_order("0,1,2,3,4"), Some([0, 1, 2, 3, 4]));
        assert_eq!(parse_order("4, 3,2,1,0"), Some([4, 3, 2, 1, 0]));
        assert_eq!(parse_order("0,1,2,3"), None);
        assert_eq!(parse_order("0,1,2,3,3"), None);
        assert_eq!(parse_order("0,1,2,3,5"), None);
        assert_eq!(parse_order("a,b,c,d,e"), None);
    }

    #[test]
    fn rejects_everything_else() {
        for bad in [
            "",
            "LTAC2|a|b",
            "LTACD2|HB|1|1|en|red",
            "LTACD1|HB|x|1|en|red",
            "LTACD1|HB|1|2|en|red",
            "LTACD1|HB|1|1|en",
            "LTACD1|HB|1|1|en|green",
            "LTACD1|HB|1|1|en|red|2",
            "LTACD1|HB|1|1|en|red|1|extra",
            "LTACD1|SWAP|1|swap/red/a,b,c,d,e|0,1,2,3",
            "LTACD1|SWAP|1|swap/red/a,b,c,d,e|0,1,2,3,3",
            "LTACD1|SWAP|1|swap/red/a,b,c,d,e|0,1,2,3,5",
            "LTACD1|SWAP|1|red/a,b,c,d,e|0,1,2,3,4",
            "LTACD1|SWAP|1|swap/red/a,b,c,d,e|0,1,2,3,4|x",
            "LTACD1|REC|1||a|b",
            "LTACD1|REC|1|red///|a",
            "LTACD1|REC|1|red///|a|b|c",
            "LTACD1|NOPE|1",
        ] {
            assert_eq!(parse(bad), None, "{bad:?} should be rejected");
        }
    }
}

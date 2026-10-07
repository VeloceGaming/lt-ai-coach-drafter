//! The texts this mod shows.
//!
//! Two different languages are in play, on purpose:
//!
//! * the **buttons and the delegation overlay** follow the *game's* language, because
//!   they sit next to the game's own buttons. The game's language is found by asking the
//!   game for its own "Delegate ban/pick to Head Coach" string (`StableClient::i18n`) and
//!   recognising it among the game's 17 languages; the wording below is the game's own
//!   with "Head Coach" replaced by "LT Coach";
//! * the **warning panel** explains where to find a switch in the *Coach app*, so it
//!   follows the Coach's language (reported in its heartbeat) and names that app's real
//!   setting labels. Only en, ko, zh-Hans and zh-Hant exist there; anything else gets
//!   English, which is also the Coach app's own fallback.

pub struct Texts {
    /// The button over (or replacing) the game's delegate button.
    pub cover: &'static str,
    /// The overlay shown while delegation runs.
    pub overlay: &'static str,
    /// The cover over the swap stage's Coach button.
    pub swap_cover: &'static str,
    /// Under the game's "Finish Ban/Pick Now" button (0.6.3+): that button finishes the
    /// draft without asking LT Coach.
    pub instant_warning: &'static str,
    /// The warning shown when the button is pressed with LT Coach drafting off.
    pub warn_title: &'static str,
    pub warn_line1: &'static str,
    pub warn_line2: &'static str,
    pub warn_line3: &'static str,
    pub ok: &'static str,
}

struct Buttons {
    /// The game's language id (the key in its `ui.i18n`).
    id: &'static str,
    /// The game's own `banpick.delegate_to_coach` text in that language.
    signature: &'static str,
    cover: &'static str,
    swap_cover: &'static str,
    overlay: &'static str,
    instant_warning: &'static str,
}

const BUTTONS: [Buttons; 17] = [
    Buttons { id: "ko", signature: "밴픽을 수석코치에게 위임", cover: "밴픽을 LT Coach에게 위임", swap_cover: "스왑을 LT Coach에게 위임", overlay: "LT Coach가 밴픽을 진행중입니다...", instant_warning: "\"밴픽 즉시 완료\"를 누르면 남은 밴픽은 LT Coach가 아닌 게임의 수석코치가 진행합니다." },
    Buttons { id: "en", signature: "Delegate ban/pick to Head Coach", cover: "Delegate ban/pick to LT Coach", swap_cover: "Delegate swap to LT Coach", overlay: "LT Coach is running ban/pick...", instant_warning: "\"Finish Ban/Pick Now\" hands the rest of the draft to the game's Head Coach, not LT Coach." },
    Buttons { id: "ja", signature: "バンピックをヘッドコーチに委任", cover: "バンピックをLT Coachに委任", swap_cover: "スワップをLT Coachに委任", overlay: "LT Coachがバンピック中です...", instant_warning: "「バンピックを即時完了」を押すと、残りはLT Coachではなくゲームのヘッドコーチが行います。" },
    Buttons { id: "de", signature: "Bann/Auswahl an Haupttrainer delegieren", cover: "Bann/Auswahl an LT Coach delegieren", swap_cover: "Tausch an LT Coach delegieren", overlay: "LT Coach führt Bann/Auswahl durch...", instant_warning: "„Bann/Auswahl sofort beenden“ überlässt den Rest dem Haupttrainer des Spiels, nicht LT Coach." },
    Buttons { id: "vi", signature: "Ủy quyền Cấm/Chọn cho HLV trưởng", cover: "Ủy quyền Cấm/Chọn cho LT Coach", swap_cover: "Ủy quyền đổi Tướng cho LT Coach", overlay: "LT Coach đang tiến hành giai đoạn Cấm/Chọn...", instant_warning: "\"Hoàn Tất Cấm/Chọn Ngay\" sẽ giao phần còn lại cho HLV trưởng của game, không phải LT Coach." },
    Buttons { id: "pt-BR", signature: "Delegar banimento/escolha ao Treinador Principal", cover: "Delegar banimento/escolha ao LT Coach", swap_cover: "Delegar troca ao LT Coach", overlay: "O LT Coach está realizando o banimento/escolha...", instant_warning: "\"Concluir Draft Agora\" entrega o restante ao Treinador Principal do jogo, não ao LT Coach." },
    Buttons { id: "tr", signature: "Yasak/Seçmeyi Baş Antrenöre devret", cover: "Yasak/Seçmeyi LT Coach'a devret", swap_cover: "Değişimi LT Coach'a devret", overlay: "LT Coach yasak/seçme işlemini yürütüyor...", instant_warning: "\"Yasak/Seçmeyi Hemen Bitir\" kalanını LT Coach'a değil, oyunun Baş Antrenörüne bırakır." },
    Buttons { id: "nl", signature: "Ban/pick delegeren aan Hoofdtrainer", cover: "Ban/pick delegeren aan LT Coach", swap_cover: "Ruil delegeren aan LT Coach", overlay: "LT Coach voert ban/pick uit...", instant_warning: "\"Ban/pick direct afronden\" laat de rest over aan de Hoofdtrainer van het spel, niet aan LT Coach." },
    Buttons { id: "ru", signature: "Делегировать бан/пик главному тренеру", cover: "Делегировать бан/пик LT Coach", swap_cover: "Делегировать обмен LT Coach", overlay: "LT Coach проводит драфт...", instant_warning: "«Завершить бан/пик сразу» передаёт остаток драфта главному тренеру игры, а не LT Coach." },
    Buttons { id: "es-ES", signature: "Delegar bloqueo/selección al Entrenador Principal", cover: "Delegar bloqueo/selección a LT Coach", swap_cover: "Delegar intercambio a LT Coach", overlay: "LT Coach está realizando bloqueo/selección...", instant_warning: "\"Completar Draft Ya\" deja el resto al Entrenador Principal del juego, no a LT Coach." },
    Buttons { id: "fr", signature: "Déléguer le bannissement/sélection à l'Entraîneur Principal", cover: "Déléguer le bannissement/sélection à LT Coach", swap_cover: "Déléguer l'échange à LT Coach", overlay: "LT Coach effectue le bannissement/sélection...", instant_warning: "« Terminer le draft » confie la suite à l'Entraîneur Principal du jeu, pas à LT Coach." },
    Buttons { id: "it", signature: "Delega ban/pick all'Allenatore Principale", cover: "Delega ban/pick a LT Coach", swap_cover: "Delega scambio a LT Coach", overlay: "LT Coach sta gestendo il ban/pick...", instant_warning: "\"Completa Ban/Pick Ora\" affida il resto all'Allenatore Principale del gioco, non a LT Coach." },
    Buttons { id: "zh-hans", signature: "将禁用/选择委托给主教练", cover: "将禁用/选择委托给LT Coach", swap_cover: "将交换委托给LT Coach", overlay: "LT Coach正在进行禁用/选择...", instant_warning: "“立即完成禁选”会把剩余的禁选交给游戏的主教练，而不是LT Coach。" },
    Buttons { id: "zh-hant", signature: "將Ban/Pick委託給主教練", cover: "將Ban/Pick委託給LT Coach", swap_cover: "將交換委託給LT Coach", overlay: "LT Coach正在進行Ban/Pick...", instant_warning: "「立即完成Ban/Pick」會把剩下的Ban/Pick交給遊戲的主教練，而不是LT Coach。" },
    Buttons { id: "pl", signature: "Deleguj ban/picki do głównego trenera", cover: "Deleguj ban/picki do LT Coach", swap_cover: "Deleguj swapy do LT Coach", overlay: "LT Coach prowadzi ban/picki...", instant_warning: "„Zakończ ban/picki teraz” oddaje resztę głównemu trenerowi gry, a nie LT Coach." },
    Buttons { id: "th", signature: "มอบหมายสิทธิ์ขั้นตอนการดราฟต์แบน/เลือกให้โค้ชผู้ช่วยดูแลแทนทั้งหมด", cover: "มอบหมายสิทธิ์ขั้นตอนการดราฟต์แบน/เลือกให้ LT Coach ดูแลแทนทั้งหมด", swap_cover: "มอบหมายหน้าที่สลับตัวให้ LT Coach จัดการแทน", overlay: "LT Coach กำลังดำเนินการดราฟต์แบน/เลือกแชมเปียนแทนหน้างาน...", instant_warning: "\"จบการดราฟต์ทันที\" จะให้โค้ชของเกมดราฟต์ส่วนที่เหลือแทน ไม่ใช่ LT Coach" },
    Buttons { id: "haw", signature: "pāpā-koho kumu poʻo kūlana", cover: "Delegate ban/pick to LT Coach", swap_cover: "Delegate swap to LT Coach", overlay: "LT Coach is running ban/pick...", instant_warning: "\"Finish Ban/Pick Now\" hands the rest of the draft to the game's Head Coach, not LT Coach." },
];

/// Which of the game's languages produced this text, if it is the game's own
/// `banpick.delegate_to_coach` string in one of them.
pub fn detect_game_language(delegate_text: &str) -> Option<&'static str> {
    let text = delegate_text.trim();
    BUTTONS.iter().find(|b| b.signature == text).map(|b| b.id)
}

fn buttons_for(language: &str) -> Option<&'static Buttons> {
    let tag = language.to_ascii_lowercase();
    // Exact id first, then a language prefix ("pt" -> pt-BR, "es" -> es-ES, "zh-hant-tw" ...).
    BUTTONS
        .iter()
        .find(|b| b.id.to_ascii_lowercase() == tag)
        .or_else(|| {
            let base = tag.split(['-', '_']).next().unwrap_or("");
            if base == "zh" {
                // Traditional unless the tag says otherwise.
                let traditional = tag.contains("hant") || tag.ends_with("tw") || tag.ends_with("hk");
                BUTTONS.iter().find(|b| b.id == if traditional { "zh-hant" } else { "zh-hans" })
            } else {
                BUTTONS.iter().find(|b| b.id.to_ascii_lowercase().split('-').next() == Some(base))
            }
        })
}

/// The Coach-language warning, or English.
fn warning(tag: &str) -> (&'static str, &'static str, &'static str, &'static str, &'static str) {
    let tag = tag.to_ascii_lowercase();
    if tag.starts_with("ko") {
        (
            "LT Coach 위임이 꺼져 있습니다",
            "LT AI Coach 앱을 열고 설정으로 이동하세요.",
            "\"LT Coach에게 위임\"에서 \"게임에서 LT Coach가 밴/픽 진행\"을 켜세요.",
            "그런 다음 이 버튼을 다시 누르세요.",
            "확인",
        )
    } else if tag.contains("hant") || tag == "zh-tw" || tag == "zh-hk" {
        (
            "LT Coach 委託功能已關閉",
            "請開啟 LT AI Coach 應用程式並前往「設定」。",
            "在「委託給LT Coach」中開啟「讓LT Coach在遊戲中進行Ban/Pick」。",
            "然後再按一次此按鈕。",
            "確定",
        )
    } else if tag.starts_with("zh") {
        (
            "LT Coach 委托功能已关闭",
            "请打开 LT AI Coach 应用并前往“设置”。",
            "在“委托给LT Coach”中开启“让LT Coach在游戏中进行Ban/Pick”。",
            "然后再按一次此按钮。",
            "确定",
        )
    } else {
        (
            "LT Coach drafting is turned off",
            "Open the LT AI Coach app and go to Settings.",
            "Under \"Delegate to LT Coach\", turn on \"Let LT Coach draft in game\".",
            "Then press this button again.",
            "OK",
        )
    }
}

/// The texts for a given Coach language alone (used when the game's language is not known).
pub fn texts(coach_language: &str) -> Texts {
    compose(None, coach_language)
}

/// Buttons and overlay in the game's language when it is known, else in the Coach's
/// language, else English; the warning always in the Coach's language.
pub fn compose(game_language: Option<&str>, coach_language: &str) -> Texts {
    let english = &BUTTONS[1];
    let buttons = game_language
        .and_then(buttons_for)
        .or_else(|| buttons_for(coach_language))
        .unwrap_or(english);
    let (warn_title, warn_line1, warn_line2, warn_line3, ok) = warning(coach_language);
    Texts {
        cover: buttons.cover,
        overlay: buttons.overlay,
        swap_cover: buttons.swap_cover,
        instant_warning: buttons.instant_warning,
        warn_title,
        warn_line1,
        warn_line2,
        warn_line3,
        ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_where_the_table_says_it_is() {
        assert_eq!(BUTTONS[1].id, "en");
    }

    #[test]
    fn every_game_language_has_complete_button_texts_that_name_lt_coach() {
        for b in &BUTTONS {
            for text in [b.cover, b.swap_cover, b.overlay, b.signature] {
                assert!(!text.is_empty(), "{} has an empty text", b.id);
            }
            for text in [b.cover, b.swap_cover, b.overlay, b.instant_warning] {
                assert!(text.contains("LT Coach"), "{}: {text:?} does not name LT Coach", b.id);
            }
        }
    }

    #[test]
    fn the_game_language_is_recognised_from_its_own_string() {
        assert_eq!(detect_game_language("Delegate ban/pick to Head Coach"), Some("en"));
        assert_eq!(detect_game_language("バンピックをヘッドコーチに委任"), Some("ja"));
        assert_eq!(detect_game_language("將Ban/Pick委託給主教練"), Some("zh-hant"));
        assert_eq!(detect_game_language("将禁用/选择委托给主教练"), Some("zh-hans"));
        assert_eq!(detect_game_language("  Delegate ban/pick to Head Coach \n"), Some("en"));
        assert_eq!(detect_game_language("something else"), None);
        assert_eq!(detect_game_language(""), None);
    }

    #[test]
    fn signatures_are_unique_so_detection_cannot_be_ambiguous() {
        for (i, a) in BUTTONS.iter().enumerate() {
            for b in &BUTTONS[i + 1..] {
                assert_ne!(a.signature, b.signature, "{} and {} share a signature", a.id, b.id);
            }
        }
    }

    #[test]
    fn signatures_match_the_games_own_text_files_when_they_are_available() {
        // Only on a machine that has the game's unpacked assets (the developer's).
        let path = std::path::Path::new("D:/LTTM2/base_unpacked/text/ui.i18n");
        let Ok(file) = std::fs::read_to_string(path) else { return };
        for b in &BUTTONS {
            assert!(
                file.contains(&format!("\"delegate_to_coach\": \"{}\"", b.signature.replace('"', "\\\""))),
                "{}: the game's text no longer says {:?}",
                b.id,
                b.signature
            );
        }
    }

    /// The warning quotes the game's own name for the button, in every language that
    /// uses the game's wording (only where the 0.6.3 text file has been extracted).
    #[test]
    fn the_instant_warning_quotes_the_games_own_button_text_when_available() {
        let path = std::path::Path::new("D:/LTTM2/lt_swap_re/assets-0.6.3/text__ui.i18n");
        let Ok(file) = std::fs::read_to_string(path) else { return };
        for b in BUTTONS.iter().filter(|b| b.id != "haw") {
            let quoted = b.instant_warning.split(['"', '「', '」', '“', '”', '„', '«', '»']).nth(1).unwrap_or("");
            let quoted = quoted.trim();
            assert!(
                file.contains(&format!("\"coach_delegation_instant\": \"{quoted}\"")),
                "{}: {quoted:?} is not the game's own button text",
                b.id
            );
        }
    }

    #[test]
    fn game_language_wins_for_buttons_and_coach_language_for_the_warning() {
        let t = compose(Some("ja"), "zh-Hant");
        assert_eq!(t.cover, "バンピックをLT Coachに委任");
        assert_eq!(t.swap_cover, "スワップをLT Coachに委任");
        assert_eq!(t.warn_title, "LT Coach 委託功能已關閉");
    }

    #[test]
    fn an_unknown_game_language_falls_back_to_the_coachs_then_english() {
        assert_eq!(compose(None, "ko").cover, "밴픽을 LT Coach에게 위임");
        assert_eq!(compose(Some("klingon"), "ko").cover, "밴픽을 LT Coach에게 위임");
        assert_eq!(compose(None, "").cover, "Delegate ban/pick to LT Coach");
        assert_eq!(compose(None, "xx").cover, "Delegate ban/pick to LT Coach");
    }

    #[test]
    fn coach_language_tags_map_to_game_languages() {
        assert_eq!(texts("zh-Hant").cover, "將Ban/Pick委託給LT Coach");
        assert_eq!(texts("zh-hans").cover, "将禁用/选择委托给LT Coach");
        assert_eq!(texts("zh").cover, "将禁用/选择委托给LT Coach");
        assert_eq!(texts("pt").cover, "Delegar banimento/escolha ao LT Coach");
        assert_eq!(texts("es").swap_cover, "Delegar intercambio a LT Coach");
        assert_eq!(texts("ja-JP").overlay, "LT Coachがバンピック中です...");
    }

    #[test]
    fn every_language_has_a_complete_warning() {
        for tag in ["en", "zh-Hant", "zh-Hans", "ko", "ja", ""] {
            let t = texts(tag);
            for line in [t.warn_title, t.warn_line1, t.warn_line2, t.warn_line3, t.ok] {
                assert!(!line.is_empty(), "{tag} has an empty warning line");
            }
        }
    }

    /// The warning tells the user which Coach setting to switch on, so it must
    /// quote that setting's real heading and label.
    #[test]
    fn the_warning_quotes_the_coachs_real_setting_labels() {
        // Only next to a checkout of the Coach app (the developer's machine).
        let coach = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../LT_AiCoach");
        if !coach.exists() {
            return;
        }
        let cases = [
            ("en", coach.join("src/ui/lib/i18n/strings.ts")),
            ("zh-Hant", coach.join("translations/zh-hant/base.json")),
            ("zh-Hans", coach.join("translations/zh-hans/base.json")),
            ("ko", coach.join("translations/ko/base.json")),
        ];
        for (tag, path) in cases {
            let file = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {path:?}: {e}"));
            let value = |key: &str| -> String {
                let start = file
                    .find(&format!("\"{key}\": \""))
                    .unwrap_or_else(|| panic!("{tag}: {key} missing from {path:?}"))
                    + key.len()
                    + 5;
                let rest = &file[start..];
                rest[..rest.find("\",").expect("value end")].to_string()
            };
            let heading = value("settings.delegateToCoach.heading");
            let label = value("settings.delegateToCoach.label");
            let line = texts(tag).warn_line2;
            assert!(line.contains(&heading), "{tag}: warning does not quote the heading {heading:?}");
            assert!(line.contains(&label), "{tag}: warning does not quote the label {label:?}");
        }
    }
}

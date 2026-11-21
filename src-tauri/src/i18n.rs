//! The language of text the backend shows: notifications, the tray menu, job progress and error
//! messages. One of the UI languages, from `general.language` ("system" follows the Windows
//! display language). Strings sit at their call sites through `tr!`, one literal per language.
//! The frontend has its own list (src/lib/i18n.svelte.ts); add a language in both.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    ZhCn,
    ZhTw,
    Es,
    Fr,
    Ja,
    Ko,
    Ru,
}

/// Setting value ("zh-CN") of each language, in `Lang` order.
const CODES: [&str; 8] = ["en", "zh-CN", "zh-TW", "es", "fr", "ja", "ko", "ru"];
const ALL: [Lang; 8] = [Lang::En, Lang::ZhCn, Lang::ZhTw, Lang::Es, Lang::Fr, Lang::Ja, Lang::Ko, Lang::Ru];

static LANG: AtomicU8 = AtomicU8::new(0);

/// Apply a `general.language` setting value.
pub fn set(language: &str) {
    LANG.store(from_code(resolve(language)) as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    ALL[LANG.load(Ordering::Relaxed) as usize]
}

/// The language code ("en", "zh-TW") a setting value means.
pub fn resolve(language: &str) -> &'static str {
    match CODES.iter().find(|c| **c == language) {
        Some(c) => c,
        None => CODES[system_language() as usize],
    }
}

fn from_code(code: &str) -> Lang {
    CODES.iter().position(|c| *c == code).map_or(Lang::En, |i| ALL[i])
}

/// The macOS language (the first preferred language), when the app has it.
#[cfg(target_os = "macos")]
fn system_language() -> Lang {
    let first = objc2_foundation::NSLocale::preferredLanguages().firstObject().map(|l| l.to_string()).unwrap_or_default();
    from_tag(&first)
}

/// A BCP 47 tag ("zh-Hant-HK", "fr-CA", "en") as one of the app's languages.
#[cfg_attr(windows, allow(dead_code))]
fn from_tag(tag: &str) -> Lang {
    let parts: Vec<String> = tag.split(['-', '_']).map(str::to_ascii_lowercase).collect();
    let has = |p: &str| parts.iter().skip(1).any(|x| x == p);
    match parts.first().map(String::as_str).unwrap_or("") {
        "zh" if has("hant") || ((has("tw") || has("hk") || has("mo")) && !has("hans")) => Lang::ZhTw,
        "zh" => Lang::ZhCn,
        "es" => Lang::Es,
        "fr" => Lang::Fr,
        "ja" => Lang::Ja,
        "ko" => Lang::Ko,
        "ru" => Lang::Ru,
        _ => Lang::En,
    }
}

/// The Windows display language, when the app has it.
#[cfg(windows)]
fn system_language() -> Lang {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    let id = unsafe { GetUserDefaultUILanguage() };
    // A LANGID is a primary language (low 10 bits) and a sublanguage (high 6 bits).
    match (id & 0x3ff, id >> 10) {
        // Chinese: Traditional for Taiwan (1), Hong Kong (3) and Macao (5), else Simplified.
        (0x04, 1 | 3 | 5) => Lang::ZhTw,
        (0x04, _) => Lang::ZhCn,
        (0x0a, _) => Lang::Es,
        (0x0c, _) => Lang::Fr,
        (0x11, _) => Lang::Ja,
        (0x12, _) => Lang::Ko,
        (0x19, _) => Lang::Ru,
        _ => Lang::En,
    }
}

/// `tr!("Saved {title}", zh: "已保存 {title}", tw: ..., es: ..., fr: ..., ja: ..., ko: ..., ru: ...)`: the
/// string for the current language, formatted with inline captures like `format!`. Every
/// language is required, so a missing translation does not compile.
#[macro_export]
macro_rules! tr {
    ($en:literal, zh: $zh:literal, tw: $tw:literal, es: $es:literal, fr: $fr:literal, ja: $ja:literal, ko: $ko:literal, ru: $ru:literal $(,)?) => {
        match $crate::i18n::lang() {
            $crate::i18n::Lang::En => format!($en),
            $crate::i18n::Lang::ZhCn => format!($zh),
            $crate::i18n::Lang::ZhTw => format!($tw),
            $crate::i18n::Lang::Es => format!($es),
            $crate::i18n::Lang::Fr => format!($fr),
            $crate::i18n::Lang::Ja => format!($ja),
            $crate::i18n::Lang::Ko => format!($ko),
            $crate::i18n::Lang::Ru => format!($ru),
        }
    };
}

/// Shared error text: used in many places.
pub fn recording_not_found() -> String {
    tr!("Recording not found",
        zh: "找不到该录像", tw: "找不到該錄影",
        es: "No se encontró la grabación", fr: "Enregistrement introuvable",
        ja: "録画が見つかりません", ko: "녹화를 찾을 수 없습니다", ru: "Запись не найдена")
}

pub fn ffmpeg_start_failed() -> String {
    tr!("failed to start ffmpeg",
        zh: "无法启动 ffmpeg", tw: "無法啟動 ffmpeg",
        es: "no se pudo iniciar ffmpeg", fr: "impossible de lancer ffmpeg",
        ja: "ffmpeg を起動できませんでした", ko: "ffmpeg를 시작할 수 없습니다", ru: "не удалось запустить ffmpeg")
}

/// A name the app gives recordings and clips ("Triple Kill"), in the current language.
fn name(en: &str) -> Option<String> {
    Some(match en {
        "Kill" => tr!("Kill", zh: "击杀", tw: "擊殺", es: "Asesinato", fr: "Élimination", ja: "キル", ko: "처치", ru: "Убийство"),
        "Double Kill" => tr!("Double Kill", zh: "双杀", tw: "雙殺", es: "Asesinato doble", fr: "Double élimination", ja: "ダブルキル", ko: "더블 킬", ru: "Двойное убийство"),
        "Triple Kill" => tr!("Triple Kill", zh: "三杀", tw: "三殺", es: "Asesinato triple", fr: "Triple élimination", ja: "トリプルキル", ko: "트리플 킬", ru: "Тройное убийство"),
        "Quadra Kill" => tr!("Quadra Kill", zh: "四杀", tw: "四殺", es: "Asesinato cuádruple", fr: "Quadruple élimination", ja: "クアドラキル", ko: "쿼드라 킬", ru: "Четверное убийство"),
        "Penta Kill" => tr!("Penta Kill", zh: "五杀", tw: "五殺", es: "Pentakill", fr: "Pentakill", ja: "ペンタキル", ko: "펜타 킬", ru: "Пентакилл"),
        "Death" => tr!("Death", zh: "阵亡", tw: "死亡", es: "Muerte", fr: "Mort", ja: "デス", ko: "죽음", ru: "Смерть"),
        "Assist" => tr!("Assist", zh: "助攻", tw: "助攻", es: "Asistencia", fr: "Assistance", ja: "アシスト", ko: "어시스트", ru: "Содействие"),
        "Ultimate" => tr!("Ultimate", zh: "大招", tw: "大絕", es: "Definitiva", fr: "Ultime", ja: "アルティメット", ko: "궁극기", ru: "Ульта"),
        "Baron" => tr!("Baron", zh: "纳什男爵", tw: "巴龍", es: "Barón", fr: "Baron", ja: "バロン", ko: "내셔 남작", ru: "Барон"),
        "Dragon" => tr!("Dragon", zh: "巨龙", tw: "巨龍", es: "Dragón", fr: "Dragon", ja: "ドラゴン", ko: "드래곤", ru: "Дракон"),
        "Herald" => tr!("Herald", zh: "峡谷先锋", tw: "預示者", es: "Heraldo", fr: "Héraut", ja: "ヘラルド", ko: "협곡의 전령", ru: "Герольд"),
        "Turret" => tr!("Turret", zh: "防御塔", tw: "砲塔", es: "Torreta", fr: "Tourelle", ja: "タワー", ko: "포탑", ru: "Башня"),
        "Inhibitor" => tr!("Inhibitor", zh: "水晶", tw: "抑制水晶", es: "Inhibidor", fr: "Inhibiteur", ja: "インヒビター", ko: "억제기", ru: "Ингибитор"),
        "Victory" => tr!("Victory", zh: "胜利", tw: "勝利", es: "Victoria", fr: "Victoire", ja: "勝利", ko: "승리", ru: "Победа"),
        "Manual capture" | "Bookmark" => tr!("Manual capture", zh: "手动捕捉", tw: "手動擷取", es: "Captura manual", fr: "Capture manuelle", ja: "手動キャプチャ", ko: "수동 캡처", ru: "Ручная запись"),
        "Highlight" => tr!("Highlight", zh: "高光", tw: "精華", es: "Momento destacado", fr: "Temps fort", ja: "ハイライト", ko: "하이라이트", ru: "Лучший момент"),
        "Exports" => tr!("Exports", zh: "导出", tw: "匯出", es: "Exportaciones", fr: "Exports", ja: "エクスポート", ko: "내보내기", ru: "Экспорт"),
        "Screenshot" => tr!("Screenshot", zh: "截图", tw: "截圖", es: "Captura", fr: "Capture d'écran", ja: "スクリーンショット", ko: "스크린샷", ru: "Снимок экрана"),
        _ => return None,
    })
}

/// A name the app gave ("Match 03", "Screen recording 01", "Triple Kill (2)") in the current
/// language. Names are stored in English (match numbering depends on it); names the user typed
/// are left alone. Mirrors `displayTitle` in the frontend.
pub fn title(t: &str) -> String {
    if lang() == Lang::En {
        return t.to_string();
    }
    // A " (2)" suffix from de-duplicated file names.
    let (base, suffix) = match t.rfind(" (") {
        Some(i) if t.ends_with(')') && t[i + 2..t.len() - 1].chars().all(|c| c.is_ascii_digit()) => (&t[..i], &t[i..]),
        _ => (t, ""),
    };
    if let Some(of) = base.strip_suffix(" supercut") {
        let of = title(of);
        let s = tr!("{of} supercut", zh: "{of} 集锦", tw: "{of} 集錦", es: "Resumen de {of}", fr: "Compilation de {of}", ja: "{of} スーパーカット", ko: "{of} 슈퍼컷", ru: "Нарезка: {of}");
        return format!("{s}{suffix}");
    }
    if let Some(of) = base.strip_suffix(" clip") {
        let of = title(of);
        let s = tr!("{of} clip", zh: "{of} 片段", tw: "{of} 片段", es: "Clip de {of}", fr: "Clip de {of}", ja: "{of} クリップ", ko: "{of} 클립", ru: "Клип: {of}");
        return format!("{s}{suffix}");
    }
    let numbered = |prefix: &str| base.strip_prefix(prefix).filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    let named = if let Some(n) = numbered("Match ") {
        tr!("Match {n}", zh: "对局 {n}", tw: "對局 {n}", es: "Partida {n}", fr: "Partie {n}", ja: "試合 {n}", ko: "게임 {n}", ru: "Матч {n}")
    } else if let Some(n) = numbered("Session ") {
        tr!("Session {n}", zh: "会话 {n}", tw: "時段 {n}", es: "Sesión {n}", fr: "Session {n}", ja: "セッション {n}", ko: "세션 {n}", ru: "Сессия {n}")
    } else if let Some(n) = numbered("Screen recording ") {
        tr!("Screen recording {n}", zh: "屏幕录制 {n}", tw: "螢幕錄製 {n}", es: "Grabación de pantalla {n}", fr: "Enregistrement d'écran {n}", ja: "画面録画 {n}", ko: "화면 녹화 {n}", ru: "Запись экрана {n}")
    } else if let Some(n) = numbered("Project ") {
        tr!("Project {n}", zh: "项目 {n}", tw: "專案 {n}", es: "Proyecto {n}", fr: "Projet {n}", ja: "プロジェクト {n}", ko: "프로젝트 {n}", ru: "Проект {n}")
    } else {
        match name(base) {
            Some(n) => n,
            None => return t.to_string(),
        }
    };
    format!("{named}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test, so the global language is not raced by parallel tests.
    #[test]
    fn titles_and_languages() {
        LANG.store(Lang::ZhCn as u8, Ordering::Relaxed);
        assert_eq!(title("Match 03"), "对局 03");
        assert_eq!(title("Triple Kill (2)"), "三杀 (2)");
        assert_eq!(title("Screen recording 12"), "屏幕录制 12");
        assert_eq!(title("Match 02 supercut"), "对局 02 集锦");
        assert_eq!(title("Match 02 clip (2)"), "对局 02 片段 (2)");
        assert_eq!(title("Match against Bob"), "Match against Bob");
        assert_eq!(title("Kill (x)"), "Kill (x)");
        LANG.store(Lang::Es as u8, Ordering::Relaxed);
        assert_eq!(title("Match 02 supercut (3)"), "Resumen de Partida 02 (3)");
        LANG.store(Lang::En as u8, Ordering::Relaxed);
        assert_eq!(title("Match 03"), "Match 03");

        for code in CODES {
            assert_eq!(resolve(code), code);
            assert_eq!(CODES[from_code(code) as usize], code);
        }
    }

    #[test]
    fn macos_language_tags() {
        for (tag, want) in [
            ("zh-Hans-CN", Lang::ZhCn),
            ("zh-Hant-TW", Lang::ZhTw),
            ("zh-HK", Lang::ZhTw),
            ("zh-Hans-HK", Lang::ZhCn),
            ("zh-CN", Lang::ZhCn),
            ("fr-CA", Lang::Fr),
            ("ja", Lang::Ja),
            ("ko-KR", Lang::Ko),
            ("ru-RU", Lang::Ru),
            ("es-419", Lang::Es),
            ("de-DE", Lang::En),
            ("", Lang::En),
        ] {
            assert_eq!(from_tag(tag) as u8, want as u8, "{tag}");
        }
    }
}

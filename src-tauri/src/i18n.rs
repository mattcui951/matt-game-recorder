//! The language of text the backend shows: notifications, the tray menu, job progress and error
//! messages. English or Simplified Chinese, from `general.language` ("system" follows the
//! Windows display language). Strings sit at their call sites as pairs, through `tr!`.

use std::sync::atomic::{AtomicBool, Ordering};

static ZH: AtomicBool = AtomicBool::new(false);

/// Apply a `general.language` setting value.
pub fn set(language: &str) {
    ZH.store(resolve(language) == "zh-CN", Ordering::Relaxed);
}

/// "en" or "zh-CN" for a setting value.
pub fn resolve(language: &str) -> &'static str {
    match language {
        "zh-CN" => "zh-CN",
        "en" => "en",
        _ if windows_is_chinese() => "zh-CN",
        _ => "en",
    }
}

pub fn zh() -> bool {
    ZH.load(Ordering::Relaxed)
}

fn windows_is_chinese() -> bool {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    // Primary language id of the display language; LANG_CHINESE is 0x04.
    unsafe { GetUserDefaultUILanguage() & 0x3ff == 0x04 }
}

/// `tr!("Saved {title}", "已保存 {title}")`: the string for the current language, formatted
/// with inline captures like `format!`.
#[macro_export]
macro_rules! tr {
    ($en:literal, $zh:literal $(,)?) => {
        if $crate::i18n::zh() { format!($zh) } else { format!($en) }
    };
}

/// Names the app gives recordings and clips, and their Chinese.
const NAMES: [(&str, &str); 19] = [
    ("Kill", "击杀"),
    ("Double Kill", "双杀"),
    ("Triple Kill", "三杀"),
    ("Quadra Kill", "四杀"),
    ("Penta Kill", "五杀"),
    ("Death", "阵亡"),
    ("Assist", "助攻"),
    ("Ultimate", "大招"),
    ("Baron", "纳什男爵"),
    ("Dragon", "巨龙"),
    ("Herald", "峡谷先锋"),
    ("Turret", "防御塔"),
    ("Inhibitor", "水晶"),
    ("Victory", "胜利"),
    ("Manual capture", "手动捕捉"),
    ("Bookmark", "手动捕捉"),
    ("Highlight", "高光"),
    ("Exports", "导出"),
    ("Screenshot", "截图"),
];

/// A name the app gave ("Match 03", "Screen recording 01", "Triple Kill (2)") in the current
/// language. Names are stored in English (match numbering depends on it); names the user typed
/// are left alone. Mirrors `displayTitle` in the frontend.
pub fn title(t: &str) -> String {
    if !zh() {
        return t.to_string();
    }
    // A " (2)" suffix from de-duplicated file names.
    let (base, suffix) = match t.rfind(" (") {
        Some(i) if t.ends_with(')') && t[i + 2..t.len() - 1].chars().all(|c| c.is_ascii_digit()) => (&t[..i], &t[i..]),
        _ => (t, ""),
    };
    for (en, zh) in [("Match ", "对局 "), ("Session ", "会话 "), ("Screen recording ", "屏幕录制 "), ("Project ", "项目 ")] {
        if let Some(n) = base.strip_prefix(en) {
            if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                return format!("{zh}{n}{suffix}");
            }
        }
    }
    match NAMES.iter().find(|(en, _)| *en == base) {
        Some((_, zh)) => format!("{zh}{suffix}"),
        None => t.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_in_chinese() {
        ZH.store(true, Ordering::Relaxed);
        assert_eq!(title("Match 03"), "对局 03");
        assert_eq!(title("Triple Kill (2)"), "三杀 (2)");
        assert_eq!(title("Screen recording 12"), "屏幕录制 12");
        assert_eq!(title("Match against Bob"), "Match against Bob");
        assert_eq!(title("Kill (x)"), "Kill (x)");
        ZH.store(false, Ordering::Relaxed);
        assert_eq!(title("Match 03"), "Match 03");
    }
}

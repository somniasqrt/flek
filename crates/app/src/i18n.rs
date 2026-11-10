//! Native strings. Fixed-size pairs enforce completeness for both languages.
use flek_core::settings::Language;

#[derive(Clone, Copy)]
pub enum Key {
    Starting,
    Open,
    Login,
    Quit,
    Preferences,
    Settings,
    Importing,
    SignedOut,
    SigningIn,
    Offline,
    Directory,
    Messages,
    History,
    Files,
    Current,
    Edit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Hide,
    HideOthers,
    ShowAll,
    About,
    SignIn,
}

const STRINGS: [[&str; 2]; 27] = [
    ["Starting…", "Запуск…"],
    ["Open Flek", "Відкрити Flek"],
    ["Open at Login", "Запускати при вході"],
    ["Quit Flek", "Вийти з Flek"],
    ["Preferences…", "Налаштування…"],
    ["Settings", "Налаштування"],
    [
        "Importing the Slack desktop session…",
        "Імпортуємо сеанс Slack…",
    ],
    ["Not signed in", "Вхід не виконано"],
    ["Signing in…", "Входимо…"],
    ["offline, reconnecting…", "офлайн, відновлюємо з’єднання…"],
    [
        "syncing channels and people…",
        "синхронізуємо канали й людей…",
    ],
    ["syncing messages", "синхронізуємо повідомлення"],
    ["fetching older history", "завантажуємо давнішу історію"],
    ["downloading files", "завантажуємо файли"],
    ["up to date", "усе оновлено"],
    ["Edit", "Редагування"],
    ["Undo", "Скасувати"],
    ["Redo", "Повторити"],
    ["Cut", "Вирізати"],
    ["Copy", "Копіювати"],
    ["Paste", "Вставити"],
    ["Select All", "Вибрати все"],
    ["Hide Flek", "Сховати Flek"],
    ["Hide Others", "Сховати інші"],
    ["Show All", "Показати всі"],
    ["About Flek", "Про Flek"],
    ["Sign in to Slack", "Увійти в Slack"],
];

pub fn resolve(language: Language, system: &str) -> usize {
    usize::from(
        language == Language::Uk || language == Language::System && system.starts_with("uk"),
    )
}

pub fn t(language: Language, key: Key) -> &'static str {
    let system = system_locale();
    STRINGS[key as usize][resolve(language, &system)]
}

fn system_locale() -> String {
    static LOCALE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LOCALE
        .get_or_init(|| {
            #[cfg(target_os = "macos")]
            if let Ok(output) = std::process::Command::new("/usr/bin/defaults")
                .args(["read", "-g", "AppleLocale"])
                .output()
            {
                if output.status.success() {
                    return String::from_utf8_lossy(&output.stdout)
                        .trim()
                        .to_lowercase();
                }
            }
            std::env::var("LANG").unwrap_or_default().to_lowercase()
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_native_strings_and_resolution() {
        for pair in STRINGS {
            assert!(pair.iter().all(|s| !s.is_empty()));
        }
        assert_eq!(resolve(Language::System, "uk-UA"), 1);
        assert_eq!(resolve(Language::System, "fr-FR"), 0);
        assert_eq!(resolve(Language::En, "uk-UA"), 0);
        assert_eq!(resolve(Language::Uk, "en-US"), 1);
    }
}

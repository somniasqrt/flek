//! The identity of the installed Slack desktop app (ADR 0001): the user agent
//! it sends and the versions it is built from.
//!
//! Slack logs its own user agent (`"userAgent": "… Slack_SSB/x"`) in
//! `<profile>/logs`. Logs rotate, so when none of them matches the installed
//! version the user agent is rebuilt from the bundle: app version from
//! `Info.plist`, Electron from its framework's `Info.plist`, Chrome from the
//! framework binary, using the newest logged user agent as the template.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::api::profile::token_version;
use crate::{Error, Result};

/// Versions and user agent of the installed Slack desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopProfile {
    pub app: String,
    pub electron: String,
    pub chrome: String,
    pub user_agent: String,
    /// The user agent was logged by Slack itself, not rebuilt.
    pub from_logs: bool,
}

/// Where Slack.app may be installed (App Store build included).
pub fn app_bundles(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications/Slack.app"),
        home.join("Applications/Slack.app"),
    ]
}

/// The installed Slack.app, if any.
pub fn find_app(home: &Path) -> Option<PathBuf> {
    app_bundles(home)
        .into_iter()
        .find(|p| p.join("Contents/Info.plist").is_file())
}

/// Version of the installed Slack desktop (cheap: one plist read).
pub fn installed_version(home: &Path) -> Option<String> {
    app_version(&find_app(home)?)
}

pub fn app_version(app: &Path) -> Option<String> {
    plist_string(
        &app.join("Contents/Info.plist"),
        "CFBundleShortVersionString",
    )
}

pub fn electron_version(app: &Path) -> Option<String> {
    plist_string(
        &app.join("Contents/Frameworks/Electron Framework.framework/Resources/Info.plist"),
        "CFBundleVersion",
    )
}

/// Full Chrome version compiled into Electron (its `Chrome/x.y.z.w` string).
pub fn chrome_version(app: &Path) -> Result<Option<String>> {
    let binary = app.join("Contents/Frameworks/Electron Framework.framework/Electron Framework");
    scan_chrome_version(std::fs::File::open(binary)?)
}

/// Finds the first `Chrome/<a>.<b>.<c>.<d>` in a byte stream.
pub fn scan_chrome_version(mut reader: impl Read) -> Result<Option<String>> {
    const NEEDLE: &[u8] = b"Chrome/";
    const OVERLAP: usize = 64;
    let mut buf = vec![0u8; 1 << 20];
    let mut keep = 0;
    loop {
        let n = reader.read(&mut buf[keep..])?;
        let len = keep + n;
        let window = &buf[..len];
        let mut from = 0;
        while let Some(i) = find(&window[from..], NEEDLE) {
            let at = from + i + NEEDLE.len();
            let rest = &window[at..len.min(at + 32)];
            if let Some(v) = full_version(rest) {
                return Ok(Some(v));
            }
            from = at;
        }
        if n == 0 {
            return Ok(None);
        }
        keep = OVERLAP.min(len);
        buf.copy_within(len - keep..len, 0);
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `150.0.7871.114` at the start of `bytes`, only with exactly four parts.
fn full_version(bytes: &[u8]) -> Option<String> {
    let end = bytes
        .iter()
        .position(|b| !(b.is_ascii_digit() || *b == b'.'))?;
    let v = std::str::from_utf8(&bytes[..end]).ok()?;
    let parts: Vec<&str> = v.split('.').collect();
    (parts.len() == 4 && parts.iter().all(|p| !p.is_empty())).then(|| v.to_owned())
}

/// Every Slack desktop user agent in the `*.log` files under `logs_dir`
/// (one level of subfolders), deduplicated.
pub fn logged_user_agents(logs_dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_logs(logs_dir, 0, &mut files);
    let mut out: Vec<String> = Vec::new();
    for file in files {
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        for ua in extract_user_agents(&String::from_utf8_lossy(&bytes)) {
            if !out.contains(&ua) {
                out.push(ua);
            }
        }
    }
    out
}

fn collect_logs(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth == 0 {
            collect_logs(&path, depth + 1, out);
        } else if path.extension().is_some_and(|e| e == "log") {
            out.push(path);
        }
    }
}

/// User agents of Slack desktop (`Mozilla/5.0 … Slack_SSB/x`) in log text.
pub fn extract_user_agents(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("Mozilla/5.0 (") {
        let tail = &rest[start..];
        let end = tail.find(['"', '\n', '\r']).unwrap_or(tail.len());
        let ua = &tail[..end];
        if token_version(ua, "Slack_SSB/").is_some() {
            out.push(ua.trim_end().to_owned());
        }
        rest = &tail[end..];
    }
    out
}

/// Numeric comparison key for versions like `4.51.180`.
fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Rebuilds Slack desktop's user agent for the given versions. `template` is
/// an older user agent of the same app; without one the shape Slack desktop
/// uses on macOS is assumed. `os` is the macOS version (`27.0.1`).
pub fn build_user_agent(
    template: Option<&str>,
    app: &str,
    chrome: &str,
    electron: &str,
    os: Option<&str>,
) -> String {
    let os = os.map(|v| v.replace('.', "_"));
    match template {
        Some(t) => {
            let mut ua = replace_version(t, " Slack/", app);
            ua = replace_version(&ua, "Chrome/", chrome);
            ua = replace_version(&ua, "Electron/", electron);
            ua = replace_version(&ua, "Slack_SSB/", app);
            if let Some(os) = &os {
                ua = replace_version(&ua, "Mac OS X ", os);
            }
            ua
        }
        None => {
            let os = os.unwrap_or_else(|| "10_15_7".to_owned());
            let arch = if cfg!(target_arch = "aarch64") {
                "AppleSilicon "
            } else {
                ""
            };
            format!(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X {os}) AppleWebKit/537.36 (KHTML, like Gecko) Slack/{app} Chrome/{chrome} Electron/{electron} Safari/537.36 {arch}Sonic Slack_SSB/{app}"
            )
        }
    }
}

/// Replaces the version (digits, `.` and `_`) right after `prefix`.
fn replace_version(ua: &str, prefix: &str, new: &str) -> String {
    let Some(pos) = ua.find(prefix) else {
        return ua.to_owned();
    };
    let start = pos + prefix.len();
    let len = ua[start..]
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '_'))
        .unwrap_or(ua.len() - start);
    format!("{}{new}{}", &ua[..start], &ua[start + len..])
}

/// The running macOS version, e.g. `27.0.1`.
pub fn os_version() -> Option<String> {
    plist_string(
        Path::new("/System/Library/CoreServices/SystemVersion.plist"),
        "ProductVersion",
    )
}

/// Works out the desktop profile from the app bundle (`app`) and Slack's
/// data folder (`data_root`, holding `logs/`).
pub fn read(app: Option<&Path>, data_root: &Path) -> Result<DesktopProfile> {
    let installed = app.and_then(app_version);
    let mut logged = logged_user_agents(&data_root.join("logs"));
    logged.sort_by_key(|ua| version_key(token_version(ua, "Slack_SSB/").unwrap_or("0")));

    // Slack's own user agent for the installed version (or the newest one
    // when the bundle is not where we look).
    let exact = logged.iter().rev().find(|ua| match &installed {
        Some(v) => token_version(ua, "Slack_SSB/") == Some(v.as_str()),
        None => true,
    });
    if let Some(ua) = exact {
        let get = |p| token_version(ua, p).map(str::to_owned);
        if let (Some(app), Some(chrome), Some(electron)) =
            (get("Slack_SSB/"), get("Chrome/"), get("Electron/"))
        {
            return Ok(DesktopProfile {
                app,
                electron,
                chrome,
                user_agent: ua.clone(),
                from_logs: true,
            });
        }
    }

    let missing = |what: &str| Error::DesktopData(format!("cannot tell Slack desktop's {what}"));
    let app = app.ok_or_else(|| missing("version (Slack.app not found)"))?;
    let version = installed.ok_or_else(|| missing("version"))?;
    let electron = electron_version(app).ok_or_else(|| missing("Electron version"))?;
    let chrome = chrome_version(app)?.ok_or_else(|| missing("Chrome version"))?;
    let user_agent = build_user_agent(
        logged.last().map(String::as_str),
        &version,
        &chrome,
        &electron,
        os_version().as_deref(),
    );
    Ok(DesktopProfile {
        app: version,
        electron,
        chrome,
        user_agent,
        from_logs: false,
    })
}

fn plist_string(path: &Path, key: &str) -> Option<String> {
    let value = plist::Value::from_file(path).ok()?;
    value
        .as_dictionary()?
        .get(key)?
        .as_string()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 27_0_1) AppleWebKit/537.36 (KHTML, like Gecko) Slack/4.51.180 Chrome/150.0.7871.114 Electron/43.1.1 Safari/537.36 AppleSilicon Sonic Slack_SSB/4.51.180";

    fn write_plist(path: &Path, key: &str, value: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut dict = plist::Dictionary::new();
        dict.insert(key.into(), plist::Value::String(value.into()));
        plist::Value::Dictionary(dict).to_file_xml(path).unwrap();
    }

    fn fake_app(dir: &Path, version: &str, electron: &str, chrome: &str) -> PathBuf {
        let app = dir.join("Slack.app");
        write_plist(
            &app.join("Contents/Info.plist"),
            "CFBundleShortVersionString",
            version,
        );
        let fw = app.join("Contents/Frameworks/Electron Framework.framework");
        write_plist(
            &fw.join("Resources/Info.plist"),
            "CFBundleVersion",
            electron,
        );
        let mut bin = vec![0u8; 3 << 20];
        bin.extend_from_slice(b"Chrome/%s\0Chrome/");
        bin.extend_from_slice(chrome.as_bytes());
        bin.push(0);
        std::fs::write(fw.join("Electron Framework"), bin).unwrap();
        app
    }

    #[test]
    fn extracts_user_agents_from_log_lines() {
        let log = format!(
            "[info] x\n  \"userAgent\": \"{UA}\"\nMozilla/5.0 (Windows) Chrome/1 not slack\n"
        );
        assert_eq!(extract_user_agents(&log), vec![UA.to_owned()]);
    }

    #[test]
    fn finds_chrome_version_across_chunks() {
        let mut bin = vec![b'x'; (1 << 20) - 3];
        bin.extend_from_slice(b"Chrome/150.0.7871.114\0");
        assert_eq!(
            scan_chrome_version(&bin[..]).unwrap().as_deref(),
            Some("150.0.7871.114")
        );
        assert_eq!(scan_chrome_version(&b"Chrome/1.2"[..]).unwrap(), None);
    }

    #[test]
    fn rebuilds_from_template() {
        let ua = build_user_agent(Some(UA), "4.52.1", "151.0.1.2", "44.0.0", Some("27.1"));
        assert_eq!(
            ua,
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 27_1) AppleWebKit/537.36 (KHTML, like Gecko) Slack/4.52.1 Chrome/151.0.1.2 Electron/44.0.0 Safari/537.36 AppleSilicon Sonic Slack_SSB/4.52.1"
        );
        let fresh = build_user_agent(None, "4.51.180", "150.0.7871.114", "43.1.1", Some("27.0.1"));
        assert!(fresh.starts_with("Mozilla/5.0 (Macintosh; Intel Mac OS X 27_0_1)"));
        assert!(fresh.ends_with("Sonic Slack_SSB/4.51.180"));
    }

    #[test]
    fn prefers_slack_own_user_agent() {
        let dir = tempfile::tempdir().unwrap();
        let app = fake_app(dir.path(), "4.51.180", "43.1.1", "150.0.7871.114");
        let logs = dir.path().join("data/logs/default");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("browser1.log"),
            format!("\"userAgent\": \"{UA}\"\n"),
        )
        .unwrap();

        let p = read(Some(&app), &dir.path().join("data")).unwrap();
        assert!(p.from_logs);
        assert_eq!(p.user_agent, UA);
        assert_eq!(p.chrome, "150.0.7871.114");
        assert_eq!(p.electron, "43.1.1");
    }

    #[test]
    fn rebuilds_after_slack_update() {
        let dir = tempfile::tempdir().unwrap();
        let app = fake_app(dir.path(), "4.52.0", "44.0.1", "151.0.7900.1");
        let logs = dir.path().join("data/logs/default");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("browser.log"),
            format!("\"userAgent\": \"{UA}\"\n"),
        )
        .unwrap();

        let p = read(Some(&app), &dir.path().join("data")).unwrap();
        assert!(!p.from_logs);
        assert_eq!(p.app, "4.52.0");
        assert_eq!(p.chrome, "151.0.7900.1");
        assert!(p
            .user_agent
            .contains("Slack/4.52.0 Chrome/151.0.7900.1 Electron/44.0.1"));
        assert!(p.user_agent.ends_with("Slack_SSB/4.52.0"));
    }

    #[test]
    fn no_logs_and_no_app_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(None, dir.path()).is_err());
    }
}

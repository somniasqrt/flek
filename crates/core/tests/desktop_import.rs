//! Token and cookie extraction against synthetic Chromium data.

use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use flek_core::auth::{chromium, desktop_import};
use flek_core::{Error, Secret};
use rusty_leveldb::{Options, DB};
use sha2::{Digest, Sha256};

const CONFIG: &str = r#"{"teams":{"T0FAKE1":{"id":"T0FAKE1","name":"Fake","url":"https://fake.slack.com/","token":"xoxc-fake-1"},"T0FAKE2":{"id":"T0FAKE2","name":"Другий","url":"https://two.slack.com/","token":"xoxc-fake-2"}}}"#;

fn storage_key(origin: &str, key: &str) -> Vec<u8> {
    let mut k = format!("_{origin}").into_bytes();
    k.push(0);
    k.push(1);
    k.extend_from_slice(key.as_bytes());
    k
}

fn latin1(s: &str) -> Vec<u8> {
    let mut v = vec![1u8];
    v.extend(
        s.chars()
            .map(|c| u8::try_from(u32::from(c)).expect("latin-1 only")),
    );
    v
}

fn utf16(s: &str) -> Vec<u8> {
    let mut v = vec![0u8];
    for u in s.encode_utf16() {
        v.extend_from_slice(&u.to_le_bytes());
    }
    v
}

fn write_leveldb(entries: &[(Vec<u8>, Vec<u8>)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut db = DB::open(dir.path(), Options::default()).unwrap();
    for (k, v) in entries {
        db.put(k, v).unwrap();
    }
    db.flush().unwrap();
    db.close().unwrap();
    dir
}

#[test]
fn reads_teams_from_latin1_value() {
    let config = CONFIG.replace("Другий", "Two");
    let dir = write_leveldb(&[
        (
            storage_key("https://other.example", "localConfig_v2"),
            latin1("{}"),
        ),
        (
            storage_key("https://app.slack.com", "something_else"),
            latin1("{}"),
        ),
        (
            storage_key("https://app.slack.com", "localConfig_v2"),
            latin1(&config),
        ),
    ]);
    let teams = desktop_import::read_teams(dir.path()).unwrap();
    assert_eq!(teams.len(), 2);
    assert_eq!(teams[0].team_id, "T0FAKE1");
    assert_eq!(teams[0].token.expose(), "xoxc-fake-1");
    assert_eq!(teams[1].name, "Two");
}

#[test]
fn reads_teams_from_utf16_value() {
    let dir = write_leveldb(&[(
        storage_key("https://app.slack.com", "localConfig_v2"),
        utf16(CONFIG),
    )]);
    let teams = desktop_import::read_teams(dir.path()).unwrap();
    assert_eq!(teams[1].name, "Другий");
    assert_eq!(teams[1].token.expose(), "xoxc-fake-2");
}

#[test]
fn missing_local_config_means_not_logged_in() {
    let dir = write_leveldb(&[(storage_key("https://app.slack.com", "other"), latin1("{}"))]);
    assert!(matches!(
        desktop_import::read_teams(dir.path()),
        Err(Error::DesktopNotLoggedIn)
    ));
}

#[test]
fn ignores_the_lock_file_when_copying() {
    let dir = write_leveldb(&[(
        storage_key("https://app.slack.com", "localConfig_v2"),
        latin1(CONFIG.split("Другий").next().unwrap()),
    )]);
    let copy = chromium::copy_leveldb(dir.path()).unwrap();
    assert!(!copy.path().join("LOCK").exists());
}

const PASSWORD: &str = "fake-safe-storage-password";
const COOKIE: &str = "xoxd-FAKE%2Bcookie%3D%3D";

fn encrypt(plain: &[u8]) -> Vec<u8> {
    let key = chromium::derive_key(&Secret::new(PASSWORD));
    let ct = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &[b' '; 16].into())
        .encrypt_padded_vec_mut::<Pkcs7>(plain);
    [b"v10".as_slice(), &ct].concat()
}

fn with_host_hash(value: &str) -> Vec<u8> {
    // Make sure the first byte is non-printable, as the heuristic expects.
    let mut host = String::from(".slack.com");
    loop {
        let hash = Sha256::digest(host.as_bytes());
        if !hash[0].is_ascii_graphic() {
            return [hash.as_slice(), value.as_bytes()].concat();
        }
        host.push('x');
    }
}

#[test]
fn derives_the_known_pbkdf2_key() {
    // Reference from Python: hashlib.pbkdf2_hmac("sha1", b"peanuts", b"saltysalt", 1003, 16)
    let key = chromium::derive_key(&Secret::new("peanuts"));
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, "d9a09d499b4e1b7461f28e67972c6dbd");
}

#[test]
fn decrypts_cookie_without_host_hash() {
    let key = chromium::derive_key(&Secret::new(PASSWORD));
    let value = chromium::decrypt_cookie(&encrypt(COOKIE.as_bytes()), &key).unwrap();
    assert_eq!(value.expose(), COOKIE);
}

#[test]
fn decrypts_cookie_with_host_hash() {
    let key = chromium::derive_key(&Secret::new(PASSWORD));
    let value = chromium::decrypt_cookie(&encrypt(&with_host_hash(COOKIE)), &key).unwrap();
    assert_eq!(value.expose(), COOKIE);
}

#[test]
fn wrong_password_fails_cleanly() {
    let key = chromium::derive_key(&Secret::new("wrong"));
    let err = chromium::decrypt_cookie(&encrypt(COOKIE.as_bytes()), &key).unwrap_err();
    assert!(matches!(err, Error::CookieDecrypt(_)));
    assert!(!err.to_string().contains("xoxd"));
}

#[test]
fn rejects_unknown_version() {
    let key = chromium::derive_key(&Secret::new(PASSWORD));
    assert!(chromium::decrypt_cookie(b"v11abcdefabcdefabcdef", &key).is_err());
}

fn cookie_db(rows: &[(&str, &str, &str, Vec<u8>)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("Cookies")).unwrap();
    conn.execute_batch(
        "CREATE TABLE cookies (host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB, expires_utc INTEGER);",
    )
    .unwrap();
    for (i, (host, name, value, enc)) in rows.iter().enumerate() {
        conn.execute(
            "INSERT INTO cookies VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![host, name, value, enc, i as i64],
        )
        .unwrap();
    }
    dir
}

#[test]
fn reads_and_decrypts_d_cookie_from_sqlite() {
    let dir = cookie_db(&[
        (".example.com", "d", "", encrypt(b"xoxd-wrong-host")),
        (".slack.com", "d-s", "", encrypt(b"123")),
        (".slack.com", "d", "", encrypt(&with_host_hash(COOKIE))),
    ]);
    let mut asked = false;
    let cookie = desktop_import::read_cookie(&dir.path().join("Cookies"), || {
        asked = true;
        Ok(Secret::new(PASSWORD))
    })
    .unwrap();
    assert!(asked);
    assert_eq!(cookie.expose(), COOKIE);
}

#[test]
fn plain_cookie_skips_keychain() {
    let dir = cookie_db(&[(".slack.com", "d", COOKIE, Vec::new())]);
    let cookie = desktop_import::read_cookie(&dir.path().join("Cookies"), || {
        panic!("keychain must not be touched")
    })
    .unwrap();
    assert_eq!(cookie.expose(), COOKIE);
}

#[test]
fn missing_cookie_means_not_logged_in() {
    let dir = cookie_db(&[(".slack.com", "x", "1", Vec::new())]);
    let err =
        desktop_import::read_cookie(&dir.path().join("Cookies"), || Ok(Secret::new(PASSWORD)))
            .unwrap_err();
    assert!(matches!(err, Error::DesktopNotLoggedIn));
}

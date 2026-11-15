//! Windows cookie format (Local State key + AES-256-GCM) against synthetic data.
//! DPAPI itself is Windows-only and injected, so these run on every host.

use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD, Engine};
use flek_core::auth::chromium::{self, StoredCookie};
use flek_core::auth::desktop_import::windows::{decrypt_cookie, unwrap_key};
use flek_core::{Error, Result};
use sha2::{Digest, Sha256};

const VALUE: &str = "xoxd-synthetic%2Fcookie";
const HOST: &str = ".slack.com";

/// Independent vectors made with Node `crypto.createCipheriv('aes-256-gcm')`:
/// key 00..1f, nonce 00..0b, empty AAD.
const VECTOR_PLAIN: &str = "763130000102030405060708090a0b3f6dae7fe896bb75f929f2ffd88a5d5fc5b5e85b9b123a3a636a525564f1b4119ddf5ace25854f";
const VECTOR_HASHED: &str = "763130000102030405060708090a0bd61ea55f68b9e855e5b292b4a9274bd39c7f27c00711cc980423b90374aea2ca797fd69882b26bf600cc1a99e1e40d0aa83a0fe231bfc66967fb53fec807a71359cf21ac3fbc41";

fn key() -> Vec<u8> {
    (0u8..32).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn seal(prefix: &[u8], key: &[u8], plain: &[u8]) -> Vec<u8> {
    let nonce: Vec<u8> = (0u8..12).collect();
    let ct = Aes256Gcm::new_from_slice(key)
        .unwrap()
        .encrypt(Nonce::from_slice(&nonce), plain)
        .unwrap();
    [prefix, &nonce, &ct].concat()
}

fn hashed(host: &str, value: &str) -> Vec<u8> {
    [Sha256::digest(host.as_bytes()).as_slice(), value.as_bytes()].concat()
}

fn err_text(r: Result<impl std::fmt::Debug>) -> String {
    r.unwrap_err().to_string()
}

#[test]
fn decrypts_independent_vectors() {
    assert_eq!(
        decrypt_cookie(&unhex(VECTOR_PLAIN), &key(), HOST)
            .unwrap()
            .expose(),
        VALUE
    );
    assert_eq!(
        decrypt_cookie(&unhex(VECTOR_HASHED), &key(), HOST)
            .unwrap()
            .expose(),
        VALUE
    );
}

#[test]
fn v11_prefix_is_accepted_like_v10() {
    let mut rec = unhex(VECTOR_HASHED);
    rec[2] = b'1';
    assert_eq!(&rec[..3], b"v11");
    assert_eq!(decrypt_cookie(&rec, &key(), HOST).unwrap().expose(), VALUE);
}

#[test]
fn own_encryption_roundtrips_for_both_layouts() {
    let k = key();
    let plain = seal(b"v10", &k, VALUE.as_bytes());
    let bound = seal(b"v10", &k, &hashed(HOST, VALUE));
    assert_eq!(decrypt_cookie(&plain, &k, HOST).unwrap().expose(), VALUE);
    assert_eq!(decrypt_cookie(&bound, &k, HOST).unwrap().expose(), VALUE);
}

#[test]
fn wrong_host_hash_is_rejected() {
    let rec = seal(b"v10", &key(), &hashed("slack.com", VALUE));
    assert!(decrypt_cookie(&rec, &key(), HOST).is_err());
    // The matching host works.
    assert!(decrypt_cookie(&rec, &key(), "slack.com").is_ok());
}

#[test]
fn corruption_and_wrong_key_fail_authentication() {
    let good = unhex(VECTOR_HASHED);
    // Nonce, ciphertext and tag bytes.
    for i in [3, 14, 20, good.len() - 1] {
        let mut bad = good.clone();
        bad[i] ^= 1;
        assert!(decrypt_cookie(&bad, &key(), HOST).is_err(), "byte {i}");
    }
    let mut other = key();
    other[0] ^= 1;
    assert!(decrypt_cookie(&good, &other, HOST).is_err());
}

#[test]
fn bad_key_length_and_truncation_are_rejected() {
    let good = unhex(VECTOR_PLAIN);
    assert!(decrypt_cookie(&good, &key()[..16], HOST).is_err());
    // version only, version + partial nonce, nonce without tag, one byte short of nonce + tag.
    for len in [3, 10, 15, 3 + 12 + 15] {
        assert!(decrypt_cookie(&good[..len], &key(), HOST).is_err(), "{len}");
    }
    assert!(decrypt_cookie(b"", &key(), HOST).is_err());
}

#[test]
fn unknown_versions_are_rejected() {
    let mut rec = unhex(VECTOR_PLAIN);
    for version in [b"v20", b"v12", b"v09", b"DPA"] {
        rec[..3].copy_from_slice(version);
        assert!(decrypt_cookie(&rec, &key(), HOST).is_err());
    }
}

#[test]
fn rejects_plaintext_that_is_not_a_session_cookie() {
    let k = key();
    for plain in [
        b"".to_vec(),
        "xoxd-не-ascii".as_bytes().to_vec(),
        b"abc-not-a-session".to_vec(),
        b"xoxd-has space".to_vec(),
        b"xoxd-ctrl\x01".to_vec(),
        hashed(HOST, "plain-value"),
    ] {
        let rec = seal(b"v10", &k, &plain);
        assert!(decrypt_cookie(&rec, &k, HOST).is_err(), "{plain:?}");
    }
}

#[test]
fn decrypt_errors_never_quote_secrets() {
    let k = key();
    let rec = seal(b"v10", &k, b"xoxd-secret-plaintext\x01");
    let texts = [
        err_text(decrypt_cookie(&rec, &k, HOST)),
        err_text(decrypt_cookie(&rec[..20], &k, HOST)),
        err_text(decrypt_cookie(&unhex(VECTOR_PLAIN), &k[..8], HOST)),
    ];
    for text in texts {
        assert!(!text.contains("secret-plaintext"), "{text}");
        assert!(!text.contains("xoxd"), "{text}");
    }
}

fn local_state(wrapped: &[u8]) -> String {
    format!(
        r#"{{"os_crypt":{{"encrypted_key":"{}"}},"other":1}}"#,
        STANDARD.encode(wrapped)
    )
}

#[test]
fn unwrap_key_strips_the_marker_and_returns_the_key() {
    let protected = b"opaque-dpapi-blob";
    let state = local_state(&[b"DPAPI".as_slice(), protected].concat());
    let unwrapped = unwrap_key(&state, |blob| {
        assert_eq!(blob, protected);
        Ok(key())
    })
    .unwrap();
    assert_eq!(unwrapped.as_slice(), key().as_slice());
}

#[test]
fn unwrap_key_rejects_bad_local_state() {
    let never = |_: &[u8]| -> Result<Vec<u8>> { panic!("DPAPI must not be called") };
    for state in [
        String::new(),
        "not json".to_owned(),
        r#"{"os_crypt":{}}"#.to_owned(),
        r#"{"os_crypt":{"encrypted_key":5}}"#.to_owned(),
        r#"{"os_crypt":{"encrypted_key":"***not base64***"}}"#.to_owned(),
        local_state(b""),
        local_state(b"DPAP"),
        local_state(b"DPAPI"),
        local_state(b"xxxxxblob"),
        local_state(b"dpapiblob"),
    ] {
        assert!(unwrap_key(&state, never).is_err(), "{state}");
    }
}

#[test]
fn unwrap_key_checks_unwrap_failure_and_key_size() {
    let state = local_state(b"DPAPIblob");
    assert!(unwrap_key(&state, |_| Err(Error::CookieDecrypt("no".into()))).is_err());
    for len in [0usize, 16, 31, 33, 64] {
        assert!(unwrap_key(&state, |_| Ok(vec![7; len])).is_err(), "{len}");
    }
    assert_eq!(unwrap_key(&state, |_| Ok(vec![7; 32])).unwrap().len(), 32);
}

#[test]
fn unwrap_key_errors_never_quote_input() {
    let state = local_state(b"DPAPIsecret-blob-bytes");
    let text = err_text(unwrap_key(&state, |_| Ok(vec![1; 5])));
    assert!(!text.contains("secret-blob"), "{text}");
    assert!(!text.contains(&STANDARD.encode(b"DPAPIsecret-blob-bytes")));
}

fn cookie_db(rows: &[(&str, &str, &str, Vec<u8>, i64)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("Cookies")).unwrap();
    conn.execute_batch(
        "CREATE TABLE cookies (host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB, expires_utc INTEGER);",
    )
    .unwrap();
    for (host, name, value, enc, expires) in rows {
        conn.execute(
            "INSERT INTO cookies VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![host, name, value, enc, expires],
        )
        .unwrap();
    }
    dir
}

#[test]
fn cookie_store_reports_the_host_used_for_the_hash() {
    let k = key();
    // The host the cookie was stored for must be the one hashed into it.
    let rec = seal(b"v10", &k, &hashed("slack.com", VALUE));
    let dir = cookie_db(&[
        (".example.com", "d", "", seal(b"v10", &k, b"xoxd-other"), 1),
        (".slack.com", "d-s", "", seal(b"v10", &k, b"123"), 2),
        ("slack.com", "d", "", rec, 3),
    ]);
    let (host, stored) = chromium::read_cookie_with_host(
        &dir.path().join("Cookies"),
        &[".slack.com", "slack.com"],
        "d",
    )
    .unwrap()
    .expect("cookie row");
    assert_eq!(host, "slack.com");
    let StoredCookie::Encrypted(bytes) = stored else {
        panic!("expected an encrypted cookie");
    };
    assert_eq!(decrypt_cookie(&bytes, &k, &host).unwrap().expose(), VALUE);
}

#[test]
fn cookie_store_without_the_cookie_returns_none_and_plain_values_pass_through() {
    let dir = cookie_db(&[(".slack.com", "x", "1", Vec::new(), 1)]);
    assert!(
        chromium::read_cookie_with_host(&dir.path().join("Cookies"), &[".slack.com"], "d")
            .unwrap()
            .is_none()
    );
    let dir = cookie_db(&[(".slack.com", "d", VALUE, Vec::new(), 1)]);
    let (host, stored) =
        chromium::read_cookie_with_host(&dir.path().join("Cookies"), &[".slack.com"], "d")
            .unwrap()
            .unwrap();
    assert_eq!(host, ".slack.com");
    assert!(matches!(stored, StoredCookie::Plain(v) if v == VALUE));
}

//! Reading Chromium/Electron profile data: Local Storage (LevelDB) and the
//! encrypted cookie store. Nothing here is Slack-specific except the
//! constants callers pass in.

use std::fs;
use std::path::Path;

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use rusty_leveldb::{LdbIterator, Options, DB};
use sha1::Sha1;

use crate::{Error, Result, Secret};

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

const PBKDF2_SALT: &[u8] = b"saltysalt";
const PBKDF2_ROUNDS: u32 = 1003;
const CBC_IV: [u8; 16] = [b' '; 16];
const HOST_HASH_LEN: usize = 32;

/// Copies a LevelDB directory (minus its `LOCK`) into a temp dir, so the
/// running browser's lock does not get in the way.
pub fn copy_leveldb(src: &Path) -> Result<tempfile::TempDir> {
    let tmp = tempfile::tempdir()?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.file_name() == "LOCK" {
            continue;
        }
        fs::copy(entry.path(), tmp.path().join(entry.file_name()))?;
    }
    Ok(tmp)
}

/// Finds a Local Storage value for `origin` (e.g. `https://app.slack.com`)
/// and `key` in an already copied LevelDB directory. Returns the raw stored
/// bytes, still with Chromium's encoding prefix.
pub fn find_local_storage_value(db_dir: &Path, origin: &str, key: &str) -> Result<Option<Vec<u8>>> {
    let opts = Options {
        create_if_missing: false,
        ..Options::default()
    };
    let mut db = DB::open(db_dir, opts).map_err(|e| Error::LevelDb(e.to_string()))?;
    let mut it = db.new_iter().map_err(|e| Error::LevelDb(e.to_string()))?;

    // Chromium key layout: `_<origin>\0<encoding byte><key>`.
    let mut prefix = Vec::with_capacity(origin.len() + 2);
    prefix.push(b'_');
    prefix.extend_from_slice(origin.as_bytes());
    prefix.push(0);

    let mut found = None;
    while let Some((k, v)) = it.next() {
        if let Some(rest) = k.strip_prefix(prefix.as_slice()) {
            if decode_storage_string(rest).ok().as_deref() == Some(key) {
                found = Some(v);
            }
        }
    }
    Ok(found)
}

/// Decodes a Chromium Local Storage string: first byte `0` means UTF-16LE,
/// `1` means Latin-1. Values without a prefix are taken as UTF-8.
pub fn decode_storage_string(raw: &[u8]) -> Result<String> {
    match raw.split_first() {
        Some((0, rest)) => {
            if rest.len() % 2 != 0 {
                return Err(Error::DesktopData("odd-length UTF-16 value".into()));
            }
            let units: Vec<u16> = rest
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16(&units)
                .map_err(|_| Error::DesktopData("invalid UTF-16 value".into()))
        }
        Some((1, rest)) => Ok(rest.iter().map(|&b| char::from(b)).collect()),
        Some(_) => String::from_utf8(raw.to_vec())
            .map_err(|_| Error::DesktopData("value is not valid UTF-8".into())),
        None => Ok(String::new()),
    }
}

/// Cookie value as stored in the `cookies` table.
pub enum StoredCookie {
    Plain(String),
    Encrypted(Vec<u8>),
}

/// Reads one cookie from a Chromium `Cookies` SQLite file. The file (plus
/// any journal) is copied first; the browser may hold it open.
pub fn read_cookie(cookies_db: &Path, hosts: &[&str], name: &str) -> Result<Option<StoredCookie>> {
    let tmp = tempfile::tempdir()?;
    let file_name = cookies_db
        .file_name()
        .ok_or_else(|| Error::DesktopData("cookie store path has no file name".into()))?;
    let copy = tmp.path().join(file_name);
    fs::copy(cookies_db, &copy)?;
    for suffix in ["-journal", "-wal"] {
        let mut side = cookies_db.as_os_str().to_owned();
        side.push(suffix);
        let side = Path::new(&side);
        if side.exists() {
            let mut dst = copy.as_os_str().to_owned();
            dst.push(suffix);
            fs::copy(side, dst)?;
        }
    }

    let conn =
        rusqlite::Connection::open_with_flags(&copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare(
        "SELECT host_key, value, encrypted_value FROM cookies WHERE name = ?1 ORDER BY expires_utc DESC",
    )?;
    let mut rows = stmt.query([name])?;
    while let Some(row) = rows.next()? {
        let host: String = row.get(0)?;
        if !hosts.contains(&host.as_str()) {
            continue;
        }
        let value: String = row.get(1)?;
        if !value.is_empty() {
            return Ok(Some(StoredCookie::Plain(value)));
        }
        let encrypted: Vec<u8> = row.get(2)?;
        if !encrypted.is_empty() {
            return Ok(Some(StoredCookie::Encrypted(encrypted)));
        }
    }
    Ok(None)
}

/// macOS cookie key: PBKDF2-HMAC-SHA1 over the "Safe Storage" keychain password.
pub fn derive_key(password: &Secret) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<Sha1>(
        password.expose().as_bytes(),
        PBKDF2_SALT,
        PBKDF2_ROUNDS,
        &mut key,
    );
    key
}

/// Decrypts a `v10` cookie value. Newer Chromium prepends SHA-256 of the
/// host to the plaintext; it is stripped when present.
pub fn decrypt_cookie(encrypted: &[u8], key: &[u8; 16]) -> Result<Secret> {
    let body = encrypted
        .strip_prefix(b"v10")
        .ok_or_else(|| Error::CookieDecrypt("unknown encryption version (expected v10)".into()))?;
    let plain = Aes128CbcDec::new(key.into(), &CBC_IV.into())
        .decrypt_padded_vec_mut::<Pkcs7>(body)
        .map_err(|_| Error::CookieDecrypt("wrong key or corrupted value".into()))?;

    let value = strip_host_hash(&plain);
    if value.is_empty() || !value.iter().all(|b| b.is_ascii_graphic()) {
        return Err(Error::CookieDecrypt(
            "decrypted value is not printable".into(),
        ));
    }
    // Checked above: ASCII only.
    Ok(Secret::new(String::from_utf8_lossy(value).into_owned()))
}

fn strip_host_hash(plain: &[u8]) -> &[u8] {
    let starts_unprintable = plain.first().is_some_and(|b| !b.is_ascii_graphic());
    if plain.len() > HOST_HASH_LEN && starts_unprintable {
        &plain[HOST_HASH_LEN..]
    } else {
        plain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_latin1_and_utf16() {
        assert_eq!(
            decode_storage_string(b"\x01{\"a\":1}").unwrap(),
            "{\"a\":1}"
        );
        let mut utf16 = vec![0u8];
        for u in "{\"ї\":1}".encode_utf16() {
            utf16.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_storage_string(&utf16).unwrap(), "{\"ї\":1}");
        assert_eq!(decode_storage_string(b"{}").unwrap(), "{}");
    }

    #[test]
    fn rejects_odd_utf16() {
        assert!(decode_storage_string(&[0, 1, 2, 3]).is_err());
    }
}

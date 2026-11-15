//! Windows Chromium cookie format. Parsing and AES-GCM are portable so they can
//! be checked with synthetic inputs on any host; only DPAPI and profile I/O are
//! Windows-specific. No profile keys or decrypted values belong in diagnostics.

use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{Error, Result, Secret};

/// Decode Local State and unwrap its profile key. The injected unprotect
/// function keeps DPAPI out of pure parser tests; production uses current-user
/// DPAPI with no entropy, matching Chromium's os_crypt implementation.
pub fn unwrap_key(
    local_state: &str,
    unprotect: impl FnOnce(&[u8]) -> Result<Vec<u8>>,
) -> Result<Zeroizing<Vec<u8>>> {
    #[derive(Deserialize)]
    struct LocalState {
        os_crypt: OsCrypt,
    }
    #[derive(Deserialize)]
    struct OsCrypt {
        encrypted_key: String,
    }
    let state: LocalState = serde_json::from_str(local_state)
        .map_err(|_| Error::CookieDecrypt("Local State has no valid os_crypt key".into()))?;
    let wrapped = STANDARD
        .decode(state.os_crypt.encrypted_key)
        .map_err(|_| Error::CookieDecrypt("Local State key is not valid base64".into()))?;
    let blob = wrapped
        .strip_prefix(b"DPAPI")
        .filter(|blob| !blob.is_empty())
        .ok_or_else(|| Error::CookieDecrypt("Local State key is not DPAPI-protected".into()))?;
    let key = Zeroizing::new(unprotect(blob)?);
    if key.len() != 32 {
        return Err(Error::CookieDecrypt(
            "Local State key is not an AES-256 key".into(),
        ));
    }
    Ok(key)
}

/// `v10`/`v11` + 12-byte nonce + ciphertext + 16-byte authentication tag,
/// with empty associated data. Reject unknown formats (including app-bound
/// `v20`) instead of attempting to bypass their protection.
pub fn decrypt_cookie(encrypted: &[u8], key: &[u8], host: &str) -> Result<Secret> {
    let body = encrypted
        .strip_prefix(b"v10")
        .or_else(|| encrypted.strip_prefix(b"v11"))
        .ok_or_else(|| {
            Error::CookieDecrypt("unsupported Windows cookie encryption version".into())
        })?;
    if body.len() < 12 + 16 {
        return Err(Error::CookieDecrypt("truncated Windows cookie".into()));
    }
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| Error::CookieDecrypt("invalid Windows cookie key length".into()))?;
    let (nonce, ciphertext) = body.split_at(12);
    let plain = Zeroizing::new(
        cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| Error::CookieDecrypt("Windows cookie authentication failed".into()))?,
    );
    // Schema v24+ binds the cookie to SHA-256(host_key). Match the full digest,
    // never strip arbitrary bytes based on whether the first byte is printable.
    let hash = Sha256::digest(host.as_bytes());
    let value = plain.strip_prefix(hash.as_slice()).unwrap_or(&plain);
    // Older stores have no hash. Requiring the actual Slack session prefix also
    // rejects a wrong-host hash, even if the digest happens to be printable.
    if !value.starts_with(b"xoxd-") || !value.iter().all(u8::is_ascii_graphic) {
        return Err(Error::CookieDecrypt(
            "Windows cookie has an invalid host binding or session value".into(),
        ));
    }
    Ok(Secret::new(String::from_utf8_lossy(value).into_owned()))
}

#[cfg(target_os = "windows")]
pub(super) fn read_cookie(paths: &super::DesktopPaths) -> Result<Secret> {
    use super::super::chromium::{read_cookie_with_host, StoredCookie};

    let selected = read_cookie_with_host(&paths.cookies, super::COOKIE_HOSTS, super::COOKIE_NAME)
        .map_err(|error| match error {
            Error::Io(ref io) if io.kind() == std::io::ErrorKind::PermissionDenied => {
                Error::DesktopData(
                    "Slack's cookie store is locked; quit Slack completely and retry, or use webview sign-in".into(),
                )
            }
            other => other,
        })?;
    match selected {
        Some((_, StoredCookie::Plain(value))) => Ok(Secret::new(value)),
        Some((host, StoredCookie::Encrypted(encrypted))) => {
            let local_state =
                Zeroizing::new(std::fs::read_to_string(paths.root.join("Local State"))?);
            let key = unwrap_key(&local_state, |blob| {
                windows_dpapi::decrypt_data(blob, windows_dpapi::Scope::User, None).map_err(|_| {
                    Error::CookieDecrypt(
                        "cannot unwrap Local State key with current-user DPAPI".into(),
                    )
                })
            })?;
            decrypt_cookie(&encrypted, &key, &host)
        }
        None => Err(Error::DesktopNotLoggedIn),
    }
}

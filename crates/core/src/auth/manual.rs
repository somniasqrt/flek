//! Last-resort browser session entry. Only normalized secrets leave this module.

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use super::{CredentialStore, OriginKind, Session, SessionOrigin};
use crate::{api::ClientProfile, Error, Result, Secret};

const COOKIE_CHARS: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Parsed credentials, with the same redacted Debug behavior as stored sessions.
#[derive(Debug)]
pub struct ManualSession {
    pub token: Secret,
    pub cookie: Secret,
}

impl ManualSession {
    /// Accept a token and either a raw cookie value or a `d=…` cookie/header.
    /// Decode percent escapes once (never form-decode `+`), then encode the value
    /// consistently for the existing `Cookie: d=…` request path.
    pub fn parse(token: Secret, cookie: Secret) -> Result<Self> {
        let token_value = unquote(token.expose().trim());
        if token_value.len() > 16 * 1024
            || !token_value.strip_prefix("xoxc-").is_some_and(|suffix| {
                !suffix.is_empty()
                    && suffix
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            })
        {
            return Err(Error::Usage("manual_token_invalid".into()));
        }

        if cookie.expose().len() > 64 * 1024 {
            return Err(Error::Usage("manual_cookie_invalid".into()));
        }
        let mut value = cookie.expose().trim();
        let header = value
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("cookie:"));
        if header {
            value = value[7..].trim();
        }
        let mut found = None;
        for part in value.split(';') {
            if let Some((name, v)) = part.trim().split_once('=') {
                if name.trim() == "d" {
                    if found.is_some() {
                        return Err(Error::Usage("manual_cookie_invalid".into()));
                    }
                    found = Some(v.trim());
                }
            }
        }
        let value = match found {
            Some(v) => v,
            None if !header && !value.contains(';') => value,
            None => return Err(Error::Usage("manual_cookie_invalid".into())),
        };
        // Own the decoded copy in a zeroizing, redacted wrapper too.
        let decoded = Secret::new(
            percent_decode_str(unquote(value))
                .decode_utf8()
                .map_err(|_| Error::Usage("manual_cookie_invalid".into()))?
                .into_owned(),
        );
        if !decoded
            .expose()
            .strip_prefix("xoxd-")
            .is_some_and(|suffix| {
                !suffix.is_empty()
                    && suffix.bytes().all(|b| {
                        b.is_ascii_alphanumeric()
                            || matches!(b, b'-' | b'_' | b'.' | b'~' | b'+' | b'/' | b'=')
                    })
            })
        {
            return Err(Error::Usage("manual_cookie_invalid".into()));
        }
        Ok(Self {
            token: Secret::new(token_value),
            cookie: Secret::new(utf8_percent_encode(decoded.expose(), COOKIE_CHARS).to_string()),
        })
    }
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

/// Validate through the shared auth path, then save only the accepted session.
/// The optional CLI team selector is checked before writing credentials.
/// Errors are fixed descriptions: neither remote text nor backend diagnostics
/// can echo a pasted value across the UI/CLI boundary.
pub async fn login(
    store: &CredentialStore,
    token: Secret,
    cookie: Secret,
    team: Option<&str>,
) -> Result<Session> {
    login_with(store, token, cookie, team, super::validate).await
}

// Keep auth/save sequencing exercisable with an offline validator and MemoryBackend,
// just like the shared auth module's save_first_with/recover_with helpers.
async fn login_with<F, Fut>(
    store: &CredentialStore,
    token: Secret,
    cookie: Secret,
    team: Option<&str>,
    validate: F,
) -> Result<Session>
where
    F: FnOnce(Secret, Secret, Option<SessionOrigin>) -> Fut,
    Fut: std::future::Future<Output = Result<Session>>,
{
    let input = ManualSession::parse(token, cookie)?;
    let origin = SessionOrigin::new(OriginKind::Manual, ClientProfile::detect().user_agent);
    let session = validate(input.token, input.cookie, Some(origin))
        .await
        .map_err(validation_error)?;
    if team.is_some_and(|id| !id.eq_ignore_ascii_case(&session.credentials.team_id)) {
        return Err(Error::Usage("manual_team_mismatch".into()));
    }
    store
        .save(&session.credentials)
        .map_err(|_| Error::Keychain("manual_session_save_failed".into()))?;
    Ok(session)
}

fn validation_error(error: Error) -> Error {
    if error.is_auth_error() {
        Error::Slack {
            code: "invalid_auth".into(),
            needed: None,
        }
    } else {
        Error::Slack {
            code: "manual_validation_failed".into(),
            needed: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AuthTest;
    use crate::auth::keychain::{Credentials, MemoryBackend};
    use std::cell::Cell;

    const TOKEN: &str = "xoxc-1111-2222-abcDEF";

    fn parse(token: &str, cookie: &str) -> Result<ManualSession> {
        ManualSession::parse(Secret::new(token), Secret::new(cookie))
    }

    fn cookie(input: &str) -> String {
        parse(TOKEN, input).unwrap().cookie.expose().to_owned()
    }

    #[test]
    fn cookie_forms_normalize_to_one_encoded_value() {
        let want = "xoxd-ab%2Fc%2Bd%3D%3D";
        for input in [
            "xoxd-ab/c+d==",
            "xoxd-ab%2Fc%2Bd%3D%3D",
            "  d=xoxd-ab%2Fc%2Bd%3D%3D  ",
            "\"xoxd-ab%2Fc%2Bd%3D%3D\"",
            "d=\"xoxd-ab/c+d==\"",
            "Cookie: b=1; d=xoxd-ab%2Fc%2Bd%3D%3D; x=y",
            "cookie: d=xoxd-ab/c+d==",
            "lc=1; d=xoxd-ab%2Fc%2Bd%3D%3D",
        ] {
            assert_eq!(cookie(input), want, "{input}");
        }
        // Decoded exactly once: a double-encoded value leaves a literal `%`,
        // which a real `d` cookie never contains, so it is refused.
        assert!(parse(TOKEN, "xoxd-a%252F").is_err());
    }

    #[test]
    fn invalid_input_is_rejected_without_echo() {
        let secretish = "xoxc-SECRET-VALUE";
        for (token, cookie) in [
            ("xoxb-123", "xoxd-a"),
            ("xoxc-", "xoxd-a"),
            ("xoxc-a b", "xoxd-a"),
            (TOKEN, "xoxd-"),
            (TOKEN, "xoxs-a"),
            (TOKEN, "b=1; c=2"),
            (TOKEN, "Cookie: b=1"),
            (TOKEN, "d=xoxd-a; d=xoxd-b"),
            (TOKEN, "xoxd-a%0Ab"),
            (TOKEN, "xoxd-%FF"),
            (secretish, "xoxd-a b"),
        ] {
            let err = parse(token, cookie).unwrap_err();
            let text = format!("{err} {err:?}");
            assert!(
                !text.contains("SECRET") && !text.contains("xoxd-"),
                "{text}"
            );
        }
        assert!(parse(&format!("xoxc-{}", "a".repeat(17 * 1024)), "xoxd-a").is_err());
        assert!(parse(TOKEN, &format!("xoxd-{}", "a".repeat(65 * 1024))).is_err());
    }

    #[test]
    fn token_is_trimmed_and_unquoted_and_debug_is_redacted() {
        let s = parse("  \"xoxc-1-2-abc\"\n", "xoxd-zz").unwrap();
        assert_eq!(s.token.expose(), "xoxc-1-2-abc");
        let debug = format!("{s:?}");
        assert!(
            !debug.contains("1-2-abc") && !debug.contains("xoxd-zz"),
            "{debug}"
        );
    }

    fn session(team: &str, token: Secret, cookie: Secret) -> Session {
        Session {
            credentials: Credentials {
                team_id: team.into(),
                user_id: "U1".into(),
                token,
                cookie,
                origin: None,
            },
            identity: AuthTest {
                url: "https://example.slack.com/".into(),
                team: "Team".into(),
                user: "me".into(),
                team_id: team.into(),
                user_id: "U1".into(),
                enterprise_id: None,
            },
        }
    }

    fn store() -> CredentialStore {
        CredentialStore::with_backend(MemoryBackend::default())
    }

    #[tokio::test]
    async fn validates_normalized_values_then_saves() {
        let store = store();
        let seen = Cell::new(false);
        let out = login_with(
            &store,
            Secret::new(TOKEN),
            Secret::new("d=xoxd-a/b"),
            Some("t1"),
            |token, cookie, origin| {
                seen.set(true);
                assert_eq!(token.expose(), TOKEN);
                assert_eq!(cookie.expose(), "xoxd-a%2Fb");
                assert!(matches!(origin.unwrap().kind, OriginKind::Manual));
                async move { Ok(session("T1", token, cookie)) }
            },
        )
        .await
        .unwrap();
        assert!(seen.get());
        assert_eq!(out.credentials.team_id, "T1");
        let saved = store.load("T1").unwrap().unwrap();
        assert_eq!(saved.cookie.expose(), "xoxd-a%2Fb");
    }

    #[tokio::test]
    async fn nothing_is_saved_on_parse_auth_network_or_team_failure() {
        let store = store();
        // Parse failure never reaches the validator.
        let err = login_with(
            &store,
            Secret::new("xoxb-1"),
            Secret::new("xoxd-a"),
            None,
            |_, _, _| async { panic!("validator must not run") },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::Usage(_)));

        let auth = login_with(
            &store,
            Secret::new(TOKEN),
            Secret::new("xoxd-a"),
            None,
            |_, _, _| async {
                Err(Error::Slack {
                    code: "invalid_auth".into(),
                    needed: None,
                })
            },
        )
        .await
        .unwrap_err();
        assert!(auth.is_auth_error());

        let network = login_with(
            &store,
            Secret::new(TOKEN),
            Secret::new("xoxd-a"),
            None,
            |_, _, _| async {
                Err(Error::Slack {
                    code: "ratelimited".into(),
                    needed: None,
                })
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(network, Error::Slack { ref code, .. } if code == "manual_validation_failed")
        );

        let mismatch = login_with(
            &store,
            Secret::new(TOKEN),
            Secret::new("xoxd-a"),
            Some("T2"),
            |token, cookie, _| async move { Ok(session("T1", token, cookie)) },
        )
        .await
        .unwrap_err();
        assert!(matches!(mismatch, Error::Usage(_)));
        assert!(store.list().unwrap().is_empty(), "no credentials written");
    }
}

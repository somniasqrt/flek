# ADR 0001: Real sessions and a consistent client identity

- **Status:** accepted
- **Date:** 2026-11-08

## Context

Flek talks to Slack with the user's own session (`xoxc` token + `d` cookie),
never with an app token. Every request is visible to Slack: token, IP, user
agent, headers, TLS fingerprint, rhythm. Until now the user agent was a
hard-coded "Chrome on macOS" string, unrelated to the client that actually
issued the session (on the owner's machine: Slack desktop 4.51.180,
Electron 43.1.1, Chrome 150.0.7871.114).

Goals: always use a **real** session the user created themselves, behave
like **the client that issued it**, and make getting a session effortless.

## Decision

### 1. Where a session comes from (onboarding)

The app decides, the user never pastes tokens:

```
first launch
 ├─ Slack desktop installed and signed in? ──yes──► import (zero clicks,
 │                                                  one Keychain prompt)
 └─ no / import failed ─────────────────────────► "Sign in to Slack" window:
       embedded webview on https://app.slack.com
       (email code recommended; password logins tend to hit captchas)
         │ signed in (localConfig_v2 appears in the webview's localStorage)
         ▼
       read token from localStorage, `d` from the webview's native cookie
       store (HttpOnly, not visible to JS), navigator.userAgent
         ▼
       auth.test → Keychain → close the window → back to the main window,
       which shows "Syncing messages…" and fills in as data arrives
manual token entry: hidden under Advanced, last resort
```

The system browser is not an option: its cookies (and the HttpOnly `d`)
are out of reach.

### 2. Session origin is recorded with the session

We always know who issued a session, because we are the ones obtaining it:

| Obtained via | Issuer | Profile source |
|---|---|---|
| desktop import | Slack desktop on this machine | its own UA from its logs (`Slack/x Chrome/y Electron/z … Slack_SSB/x`); fallback: built from bundle versions (Info.plist / `app-4.x` / package) |
| webview login | Flek's webview | `navigator.userAgent` of that webview |
| manual | unknown | ask the user (browser or desktop) → browser profile for this OS |

Stored next to the credentials in Keychain (not secret, but it belongs to
the session):

```
SessionOrigin { kind: Desktop{app, electron, chrome} | Webview | Manual,
                user_agent, platform, obtained_at }
```

On start Flek compares the installed Slack desktop version with the
recorded one and refreshes the profile when Slack updated.

### 3. One profile everywhere

HTTP and websocket use the recorded profile: user agent, matching client
hints (`sec-ch-ua*`) and fetch metadata where the issuer sends them, and the
issuer's request shape (host and query parameters) once captured from
DevTools and written down in `docs/api-notes.md`. Nothing platform-specific
is hard-coded; without a recorded origin, `ClientProfile::detect()` builds a
current Chrome for the running OS.

### 4. Polite traffic (already in place)

Tier-safe pacing, a token bucket for files, websocket instead of polling,
reads from the local database only.

## Limits we accept (for now)

`reqwest` + `rustls` cannot reproduce Chrome's TLS ClientHello (JA3/JA4:
GREASE, extension set and order) or its HTTP/2 fingerprint. It *can* be
done: BoringSSL-based impersonation (`wreq`, formerly `rquest`; cf.
curl-impersonate, uTLS) mimics specific Chrome/Safari versions, websockets
included. Cost: a C dependency, profiles to keep in step with the claimed
browser version (Chrome 150 inside Electron on the owner's machine).

A different fingerprint alone says little: TLS-inspecting corporate
proxies, antivirus and VPN clients produce non-browser fingerprints for
legitimate users all the time. What stands out is a mismatch *within* one
request (UA says Chrome, TLS says rustls). Whether Slack acts on that is
unknown; the realistic worst case is a revoked session, which Flek
recovers from by re-importing.

So Flek aims to be **consistent** with the issuing client first (UA, client
hints, request shape) and keeps TLS impersonation as an opt-in transport
(cargo feature on `wreq`) to adopt if sessions start getting revoked.

## Options kept open

- **Network through the webview** (WKWebView / WebView2 `fetch` and
  websocket): genuine UA, headers and TLS. A perfect fit for webview-issued
  sessions, but moves networking out of `flek-core` and leaves the CLI
  behind. Decide during M3 once the app exists.
- **Own honest UA** (`Flek/x`): rejected; an unknown client holding a web
  session is a guaranteed anomaly.

## Consequences

- Credentials gain an origin record; `Client` is built from it.
- Webview login moves from M6 to M3 (onboarding needs it).
- A DevTools capture of the real client's requests is needed before the
  request shape can be matched.

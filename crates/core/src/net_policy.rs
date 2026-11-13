//! URL and DNS boundaries for untrusted archive/API URLs.
use crate::{Error, Result};
use reqwest::Url;
use std::net::IpAddr;

fn domain(host: &str, suffix: &str) -> bool {
    host == suffix || host.strip_suffix(suffix).is_some_and(|s| s.ends_with('.'))
}

pub(crate) fn slack_file(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && clean_authority(url)
        && url.host_str().is_some_and(|h| {
            ["slack.com", "slack-edge.com", "slack-files.com"]
                .iter()
                .any(|s| domain(h, s))
        })
}

fn clean_authority(url: &Url) -> bool {
    url.username().is_empty() && url.password().is_none()
}

pub(crate) fn same_origin(url: &Url, base: &Url) -> bool {
    clean_authority(url) && url.origin() == base.origin()
}

pub(crate) fn websocket(url: &Url, base: &Url) -> bool {
    if !clean_authority(url) {
        return false;
    }
    if url.scheme() == "wss" && url.host_str().is_some_and(|h| h.ends_with(".slack.com")) {
        return true;
    }
    let mut http_url = url.clone();
    let scheme = match url.scheme() {
        "ws" => "http",
        "wss" => "https",
        _ => return false,
    };
    if http_url.set_scheme(scheme).is_err() {
        return false;
    }
    same_origin(&http_url, base)
}

/// Session-less images (avatars, emoji, link previews) may come from any public
/// HTTPS site: unfurls point at the linked site itself. The protection is the
/// network boundary, not the host name: IP literals must be public, and every
/// name resolves through [`PublicDns`], redirects included.
pub(crate) fn public_image(url: &Url) -> Result<()> {
    let public_host = url.host_str().is_some_and(|host| {
        match host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
        {
            Ok(ip) => public_ip(ip),
            Err(_) => true,
        }
    });
    if url.scheme() != "https"
        || !clean_authority(url)
        || !public_host
        || url.port_or_known_default() != Some(443)
    {
        return Err(Error::File(
            "public image requires a public HTTPS address".into(),
        ));
    }
    Ok(())
}

/// Conservative global-unicast policy, including IPv4-mapped IPv6.
pub(crate) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, _, _] = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_broadcast()
                && !v.is_documentation()
                && !v.is_unspecified()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 192 && b == 0)
                && !(a == 198 && (18..=19).contains(&b))
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            let s = v.segments();
            // Only current global unicast; exclude special-purpose 2001::/23,
            // documentation and 6to4 (which can encode a private IPv4 endpoint).
            s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && s[1] < 0x200)
                && !(s[0] == 0x2001 && s[1] == 0xdb8)
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

#[derive(Debug)]
struct PublicDns;
impl reqwest::dns::Resolve for PublicDns {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let addresses: Vec<_> = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            if addresses.is_empty() || addresses.iter().any(|a| !public_ip(a.ip())) {
                return Err(std::io::Error::other("CDN resolved to a non-public address").into());
            }
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

pub(crate) fn image_http(user_agent: &str) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(std::time::Duration::from_secs(30))
        .no_proxy()
        // Gravatar and CDNs redirect; each hop must pass the same URL policy and
        // resolves through PublicDns again.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || public_image(attempt.url()).is_err() {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .dns_resolver(std::sync::Arc::new(PublicDns))
        .build()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_url_and_ip_policy() {
        for host in [
            "avatars.slack-edge.com",
            "emoji.slack-edge.com",
            "a.b.slack-edge.com",
            "secure.gravatar.com",
            "slack-imgs.com",
            "telegra.ph",
            "8.8.8.8",
        ] {
            assert!(public_image(&Url::parse(&format!("https://{host}/x")).unwrap()).is_ok());
        }
        for url in [
            "http://avatars.slack-edge.com/x",
            "https://127.0.0.1/x",
            "https://[::1]/x",
            "https://10.0.0.1/x",
            "https://[::ffff:192.168.1.1]/x",
            "https://slack-imgs.com:444/x",
            "https://user@slack-imgs.com/x",
        ] {
            assert!(public_image(&Url::parse(url).unwrap()).is_err(), "{url}");
        }
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.1.1",
            "192.168.1.1",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "::",
            "::1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "2002:7f00:1::",
            "2001:db8::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "2606:4700:4700::1111", "::ffff:8.8.8.8"] {
            assert!(public_ip(ip.parse().unwrap()), "{ip}");
        }
    }
    #[test]
    fn session_urls_have_exact_origin_exceptions() {
        let base = Url::parse("http://127.0.0.1:1234/api").unwrap();
        for (url, allowed) in [
            ("http://127.0.0.1:1234/f", true),
            ("http://127.0.0.1:4321/f", false),
            ("http://files.slack.com/f", false),
            ("https://files.slack.com/f", true),
            ("https://files.slack.com:443/f", true),
            ("https://files.slack.com:8443/f", false),
            ("https://a.slack-files.com/f", true),
            ("https://a.slack-edge.com/f", true),
            ("https://slack.com.evil.test/f", false),
        ] {
            let url = Url::parse(url).unwrap();
            assert_eq!(slack_file(&url) || same_origin(&url, &base), allowed);
        }
        for (url, allowed) in [
            ("ws://127.0.0.1:1234/ws", true),
            ("ws://127.0.0.1:4321/ws", false),
            ("ws://wss-primary.slack.com/ws", false),
            ("wss://wss-primary.slack.com/ws", true),
            ("wss://evil.test/ws", false),
        ] {
            assert_eq!(websocket(&Url::parse(url).unwrap(), &base), allowed);
        }
    }
}

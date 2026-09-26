//! In-app preview of the project's own dev servers.
//!
//! * [`ports`] finds the servers: URLs printed by the project's background
//!   processes, and TCP ports listened on by processes whose working folder is
//!   inside the project (Linux `/proc`).
//! * [`proxy`] opens one loopback reverse proxy per previewed server. The
//!   preview frame loads the page through it, so the picker script the proxy
//!   adds to HTML pages is same-origin with the page it inspects.
//!
//! Threat model (see also `docs/PREVIEW.md` and `SECURITY.md`):
//! * Proxies bind `127.0.0.1` on a random port and only accept requests whose
//!   `Host` is that exact address, so a web page that rebinds a DNS name to
//!   loopback cannot use them.
//! * They only forward to loopback targets (`localhost`, `*.localhost`,
//!   `127.0.0.0/8`, `::1`), never resolve names through DNS, and refuse ports
//!   this process listens on (ShadowCode's own API, MCP gateway, other proxies).
//! * The picker script is added to HTML responses only, and is served from a
//!   reserved path that is never forwarded to the dev server.
//! * The picker talks only to the window that embeds it and only to the app
//!   origin the proxy was opened for (`postMessage` with an explicit target
//!   origin; incoming messages must come from `window.parent` with that origin).
use anyhow::{bail, ensure, Context, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub mod ports;
pub mod proxy;
pub use proxy::{Opened, Previews};

/// A dev server the preview may load: a loopback host and a port.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    /// `localhost`, `<name>.localhost`, a `127.x.y.z` address or `[::1]`
    /// (IPv6 in brackets, as it appears in a URL).
    pub host: String,
    pub port: u16,
}

impl Target {
    /// Parse a preview URL. Only plain `http` to a loopback host is accepted;
    /// returns the target and the path, query and fragment to open.
    pub fn parse(url: &str) -> Result<(Self, String)> {
        let url = url.trim();
        ensure!(
            !url.is_empty(),
            "Enter a local address such as http://localhost:5173"
        );
        let parsed = reqwest::Url::parse(url)
            .context("Enter a full address such as http://localhost:5173")?;
        ensure!(
            parsed.scheme() == "http",
            "The preview opens plain http:// addresses on this computer"
        );
        ensure!(
            parsed.username().is_empty() && parsed.password().is_none(),
            "Remove the user name or password from the address"
        );
        // `host_str` is lowercased, IPv4 is normalised and IPv6 keeps its brackets.
        let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
            bail!("Enter a local address such as http://localhost:5173");
        };
        let target = Self {
            host,
            port: parsed.port_or_known_default().unwrap_or(80),
        };
        target.check()?;
        let mut rest = parsed.path().to_owned();
        if let Some(query) = parsed.query() {
            rest.push('?');
            rest.push_str(query);
        }
        if let Some(fragment) = parsed.fragment() {
            rest.push('#');
            rest.push_str(fragment);
        }
        Ok((target, rest))
    }
    /// Loopback only: names are never resolved through DNS.
    pub fn check(&self) -> Result<()> {
        ensure!(self.port != 0, "Choose a port");
        ensure!(
            !self.addresses().is_empty(),
            "The preview only opens servers on this computer (localhost, *.localhost, 127.0.0.1 or [::1])"
        );
        Ok(())
    }
    /// The loopback addresses to connect to, in order.
    pub fn addresses(&self) -> Vec<SocketAddr> {
        let host = self.host.as_str();
        let ips: Vec<IpAddr> = if host == "localhost" || is_localhost_name(host) {
            vec![Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()]
        } else if let Some(v6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            match v6.parse::<Ipv6Addr>() {
                Ok(ip) if ip.is_loopback() => vec![ip.into()],
                Ok(ip) => match ip.to_ipv4_mapped() {
                    Some(v4) if v4.is_loopback() => vec![v4.into()],
                    _ => vec![],
                },
                Err(_) => vec![],
            }
        } else {
            match host.parse::<Ipv4Addr>() {
                Ok(ip) if ip.is_loopback() => vec![ip.into()],
                _ => vec![],
            }
        };
        ips.into_iter()
            .map(|ip| SocketAddr::new(ip, self.port))
            .collect()
    }
    /// `http://host:port`, as the dev server's own pages would name it.
    pub fn origin(&self) -> String {
        format!("http://{}", self.authority())
    }
    /// `host:port`, sent as the forwarded request's `Host`.
    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// `<label>.localhost` names resolve to loopback by definition (RFC 6761).
fn is_localhost_name(host: &str) -> bool {
    host.strip_suffix(".localhost").is_some_and(|name| {
        !name.is_empty()
            && name.len() <= 200
            && name.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
    })
}

/// The app window that embeds the preview. The picker posts only to this
/// origin and obeys messages only from it, so it must be a concrete origin
/// of the desktop window or a loopback page, never `*` or `null`.
pub fn check_app_origin(origin: &str) -> Result<()> {
    let ok = match origin {
        "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost" => true,
        _ => reqwest::Url::parse(origin).is_ok_and(|url| {
            url.scheme() == "http"
                && url.origin().ascii_serialization() == origin
                && url.port().is_some()
                && url.host_str().is_some_and(|host| {
                    Target {
                        host: host.to_owned(),
                        port: url.port().unwrap_or(0),
                    }
                    .check()
                    .is_ok()
                })
        }),
    };
    ensure!(
        ok,
        "The preview can only be embedded by the ShadowCode window (origin {origin:?} is not allowed)"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_loopback_targets_only() {
        let (target, rest) = Target::parse("http://localhost:5173/settings?tab=a#x").unwrap();
        assert_eq!(target.host, "localhost");
        assert_eq!(target.port, 5173);
        assert_eq!(rest, "/settings?tab=a#x");
        assert_eq!(target.addresses().len(), 2);
        let (target, _) = Target::parse("http://App.Localhost:3000").unwrap();
        assert_eq!(target.origin(), "http://app.localhost:3000");
        let (target, _) = Target::parse("http://127.0.0.2:8080/").unwrap();
        assert_eq!(target.addresses(), vec!["127.0.0.2:8080".parse().unwrap()]);
        let (target, _) = Target::parse("http://[::1]:4000").unwrap();
        assert_eq!(target.authority(), "[::1]:4000");
        assert_eq!(target.addresses(), vec!["[::1]:4000".parse().unwrap()]);
        let (target, _) = Target::parse("http://localhost/").unwrap();
        assert_eq!(target.port, 80);
        for bad in [
            "http://example.com:5173",
            "http://10.0.0.5:3000",
            "http://192.168.1.2",
            "http://0.0.0.0:5173",
            "http://[::]:5173",
            "http://[fe80::1]:80",
            "http://localhost.example.com:80",
            "http://evil.com.localhost.:80",
            "https://localhost:5173",
            "file:///etc/passwd",
            "http://user:pw@localhost:5173",
            "localhost:5173",
            "",
        ] {
            assert!(Target::parse(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn app_origin_must_be_the_window() {
        for good in [
            "tauri://localhost",
            "http://tauri.localhost",
            "http://127.0.0.1:4178",
            "http://localhost:5174",
        ] {
            check_app_origin(good).unwrap();
        }
        for bad in [
            "*",
            "null",
            "",
            "http://evil.com",
            "http://127.0.0.1:4178/path",
            "http://127.0.0.1",
            "https://example.com:443",
        ] {
            assert!(check_app_origin(bad).is_err(), "{bad} should be refused");
        }
    }
}

//! Access tokens, one-time pairing codes and the failed-attempt limiter.
//!
//! Tokens and pairing codes are 256 random bits from the kernel. Only their
//! SHA-256 digests are kept (tokens in the 0600 profile file, pairing codes
//! in memory), and every comparison is constant-time over the digests.
use anyhow::{Context, Result};
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Read,
    net::IpAddr,
    time::{Duration, Instant},
};

/// Prefix that makes a leaked remote token easy to recognize.
pub const TOKEN_PREFIX: &str = "scr_";
/// Pairing links stay valid this long, and work once.
pub const PAIRING_TTL: Duration = Duration::from_secs(10 * 60);
/// At most this many unused pairing links exist at a time.
const MAX_PAIRING_CODES: usize = 4;
/// Failed attempts from one address before it is blocked.
pub const MAX_FAILURES: u32 = 8;
/// The window in which failures are counted, and how long a block lasts.
pub const FAILURE_WINDOW: Duration = Duration::from_secs(5 * 60);
/// Addresses tracked at once; the oldest entries are dropped beyond this.
const MAX_TRACKED: usize = 4096;

fn random_bytes() -> Result<[u8; 32]> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .context("Could not read random bytes for a remote access token")?;
    Ok(bytes)
}

/// A new access token: `scr_` and 43 URL-safe characters (256 bits).
pub fn new_token() -> Result<String> {
    Ok(format!(
        "{TOKEN_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes()?)
    ))
}

/// A new pairing code (256 bits, URL-safe, no prefix).
pub fn new_code() -> Result<String> {
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes()?))
}

pub fn digest(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in text.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}

/// Equal-length comparison whose time does not depend on where the inputs
/// differ. Different lengths compare unequal (lengths are not secret here:
/// both sides are fixed-size digests).
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let difference = left
        .iter()
        .zip(right)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    std::hint::black_box(difference) == 0
}

/// Index of the stored digest that matches `presented`. Every candidate is
/// compared, so the time taken does not reveal which one matched.
pub fn find_digest(presented: &str, stored: &[[u8; 32]]) -> Option<usize> {
    let wanted = digest(presented);
    let mut found = None;
    for (index, candidate) in stored.iter().enumerate() {
        if constant_time_eq(&wanted, candidate) && found.is_none() {
            found = Some(index);
        }
    }
    found
}

/// The token in `Authorization: Bearer <token>`, if well formed.
pub fn bearer(header: &str) -> Option<&str> {
    let (scheme, token) = header.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("Bearer")
        && (16..=256).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b)))
    .then_some(token)
}

/// Unused one-time pairing codes (digests only).
#[derive(Default)]
pub struct Pairing {
    codes: Vec<([u8; 32], Instant)>,
}
impl Pairing {
    /// A new single-use code valid for [`PAIRING_TTL`]. Older unused codes
    /// beyond [`MAX_PAIRING_CODES`] stop working.
    pub fn issue(&mut self) -> Result<String> {
        self.prune();
        let code = new_code()?;
        self.codes
            .push((digest(&code), Instant::now() + PAIRING_TTL));
        while self.codes.len() > MAX_PAIRING_CODES {
            self.codes.remove(0);
        }
        Ok(code)
    }
    /// True once for a live code; the code is then gone.
    pub fn redeem(&mut self, code: &str) -> bool {
        self.prune();
        let stored: Vec<[u8; 32]> = self.codes.iter().map(|(d, _)| *d).collect();
        match find_digest(code, &stored) {
            Some(index) => {
                self.codes.remove(index);
                true
            }
            None => false,
        }
    }
    pub fn clear(&mut self) {
        self.codes.clear();
    }
    fn prune(&mut self) {
        let now = Instant::now();
        self.codes.retain(|(_, expires)| *expires > now);
    }
}

struct Attempts {
    since: Instant,
    failures: u32,
    blocked_until: Option<Instant>,
}

/// Failed authentication and pairing attempts per client address. After
/// [`MAX_FAILURES`] failures within [`FAILURE_WINDOW`] the address is refused
/// (HTTP 429) for [`FAILURE_WINDOW`] without its credentials being checked.
#[derive(Default)]
pub struct Limiter {
    clients: HashMap<IpAddr, Attempts>,
}
impl Limiter {
    /// `Some(wait)` while `ip` is blocked.
    pub fn blocked(&mut self, ip: IpAddr) -> Option<Duration> {
        let now = Instant::now();
        let entry = self.clients.get_mut(&ip)?;
        match entry.blocked_until {
            Some(until) if until > now => Some(until - now),
            Some(_) => {
                self.clients.remove(&ip);
                None
            }
            None => None,
        }
    }
    pub fn fail(&mut self, ip: IpAddr) {
        let now = Instant::now();
        if self.clients.len() >= MAX_TRACKED && !self.clients.contains_key(&ip) {
            // Keep the table bounded: forget the oldest windows first.
            if let Some(oldest) = self
                .clients
                .iter()
                .filter(|(_, a)| a.blocked_until.is_none())
                .min_by_key(|(_, a)| a.since)
                .map(|(ip, _)| *ip)
            {
                self.clients.remove(&oldest);
            }
        }
        let entry = self.clients.entry(ip).or_insert(Attempts {
            since: now,
            failures: 0,
            blocked_until: None,
        });
        if now.duration_since(entry.since) > FAILURE_WINDOW {
            entry.since = now;
            entry.failures = 0;
        }
        entry.failures += 1;
        if entry.failures >= MAX_FAILURES {
            entry.blocked_until = Some(now + FAILURE_WINDOW);
        }
    }
    /// A successful request clears the address's failure count.
    pub fn succeed(&mut self, ip: IpAddr) {
        if self
            .clients
            .get(&ip)
            .is_some_and(|a| a.blocked_until.is_none())
        {
            self.clients.remove(&ip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_random_and_url_safe() {
        let a = new_token().unwrap();
        let b = new_token().unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with(TOKEN_PREFIX));
        assert_eq!(a.len(), TOKEN_PREFIX.len() + 43);
        assert!(bearer(&format!("Bearer {a}")).is_some());
        assert_eq!(unhex(&hex(&digest(&a))), Some(digest(&a)));
    }

    #[test]
    fn constant_time_helper_compares_whole_values() {
        assert!(constant_time_eq(b"same-value", b"same-value"));
        assert!(!constant_time_eq(b"same-value", b"same-valuf"));
        assert!(!constant_time_eq(b"xame-value", b"same-value"));
        assert!(!constant_time_eq(b"short", b"longer"));
        assert!(constant_time_eq(b"", b""));
        let stored = [digest("one"), digest("two"), digest("three")];
        assert_eq!(find_digest("two", &stored), Some(1));
        assert_eq!(find_digest("four", &stored), None);
    }

    #[test]
    fn bearer_header_parsing_is_strict() {
        assert_eq!(
            bearer("Bearer abcdefghijklmnopqr"),
            Some("abcdefghijklmnopqr")
        );
        assert_eq!(
            bearer("bearer abcdefghijklmnopqr"),
            Some("abcdefghijklmnopqr")
        );
        assert!(bearer("Basic abcdefghijklmnopqr").is_none());
        assert!(bearer("Bearer short").is_none());
        assert!(bearer("Bearer has space inside token").is_none());
        assert!(bearer("Bearer").is_none());
    }

    #[test]
    fn pairing_codes_work_once() {
        let mut pairing = Pairing::default();
        let code = pairing.issue().unwrap();
        assert!(!pairing.redeem("wrong"));
        assert!(pairing.redeem(&code));
        assert!(!pairing.redeem(&code), "a pairing link is single-use");
        let codes: Vec<_> = (0..6).map(|_| pairing.issue().unwrap()).collect();
        assert!(!pairing.redeem(&codes[0]), "old unused codes expire");
        assert!(pairing.redeem(&codes[5]));
        pairing.clear();
        assert!(!pairing.redeem(&codes[4]));
    }

    #[test]
    fn limiter_blocks_after_repeated_failures() {
        let mut limiter = Limiter::default();
        let ip: IpAddr = "192.0.2.7".parse().unwrap();
        let other: IpAddr = "192.0.2.8".parse().unwrap();
        for _ in 0..MAX_FAILURES - 1 {
            limiter.fail(ip);
            assert!(limiter.blocked(ip).is_none());
        }
        limiter.succeed(ip);
        for _ in 0..MAX_FAILURES - 1 {
            limiter.fail(ip);
        }
        assert!(limiter.blocked(ip).is_none());
        limiter.fail(ip);
        assert!(limiter.blocked(ip).is_some());
        // Success does not lift an active block; other clients are unaffected.
        limiter.succeed(ip);
        assert!(limiter.blocked(ip).is_some());
        assert!(limiter.blocked(other).is_none());
    }
}

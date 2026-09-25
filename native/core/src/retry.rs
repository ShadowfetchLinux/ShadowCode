//! Retrying a model request that failed for a passing reason: rate limits
//! (429), provider errors (5xx, "overloaded"), and connections that dropped or
//! stalled before the response finished.
//!
//! A retry re-sends the same model request. Tool calls only run after a
//! complete response has been received, so a retry never re-runs a tool: the
//! failed attempt's partial text and partial tool arguments are thrown away.
//! Waits grow exponentially with jitter, and a provider's `Retry-After` wins
//! when it is given.
use serde_json::{json, Value};
use std::time::Duration;

/// Never wait longer than this between attempts, whatever the provider asks.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);
/// Upper bound for the computed (exponential) wait.
const MAX_BACKOFF_SEC: f64 = 30.0;

/// A model request failure that carries enough detail to decide on a retry.
/// Its text is what the user sees if no retry happens.
#[derive(Debug)]
pub enum ModelFailure {
    /// The provider answered with a non-success HTTP status.
    Http {
        status: u16,
        retry_after: Option<Duration>,
        /// A local runtime on this computer (only 429/503 are retried there:
        /// other local errors are usually deterministic, like a full context).
        local: bool,
        message: String,
    },
    /// The provider sent an error object inside the stream.
    Stream {
        code: Option<u16>,
        detail: String,
        message: String,
    },
    /// The connection closed before the response finished.
    Disconnected { message: String },
    /// No bytes arrived for too long.
    Stalled { message: String },
}

impl std::fmt::Display for ModelFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http { message, .. }
            | Self::Stream { message, .. }
            | Self::Disconnected { message }
            | Self::Stalled { message } => f.write_str(message),
        }
    }
}
impl std::error::Error for ModelFailure {}

/// Why an attempt is being retried.
#[derive(Clone, Debug, PartialEq)]
pub struct Reason {
    /// `rate_limited`, `overloaded`, `server_error`, `stream_error`,
    /// `disconnected`, `stalled` or `connect_failed`.
    pub kind: &'static str,
    pub status: Option<u16>,
    pub retry_after: Option<Duration>,
}

fn status_kind(status: u16) -> Option<&'static str> {
    match status {
        429 => Some("rate_limited"),
        503 | 529 => Some("overloaded"),
        408 | 425 | 500 | 502 | 504 | 520..=528 => Some("server_error"),
        _ => None,
    }
}

/// The retryable reason behind `error`, or `None` when retrying cannot help
/// (bad key, unknown model, full context, cancellation, invalid output).
pub fn classify(error: &anyhow::Error) -> Option<Reason> {
    for cause in error.chain() {
        if let Some(failure) = cause.downcast_ref::<ModelFailure>() {
            return match failure {
                ModelFailure::Http {
                    status,
                    retry_after,
                    local,
                    ..
                } => {
                    let kind = status_kind(*status)?;
                    if *local && !matches!(status, 429 | 503) {
                        return None;
                    }
                    Some(Reason {
                        kind,
                        status: Some(*status),
                        retry_after: *retry_after,
                    })
                }
                ModelFailure::Stream { code, detail, .. } => {
                    let lower = detail.to_ascii_lowercase();
                    let kind = code.and_then(status_kind).or_else(|| {
                        [
                            "overloaded",
                            "rate limit",
                            "rate_limit",
                            "timeout",
                            "timed out",
                            "disconnected",
                            "unavailable",
                            "server_error",
                            "internal error",
                        ]
                        .iter()
                        .any(|word| lower.contains(word))
                        .then_some("stream_error")
                    })?;
                    Some(Reason {
                        kind,
                        status: *code,
                        retry_after: None,
                    })
                }
                ModelFailure::Disconnected { .. } => Some(Reason {
                    kind: "disconnected",
                    status: None,
                    retry_after: None,
                }),
                ModelFailure::Stalled { .. } => Some(Reason {
                    kind: "stalled",
                    status: None,
                    retry_after: None,
                }),
            };
        }
        if let Some(error) = cause.downcast_ref::<reqwest::Error>() {
            if error.is_connect() {
                return Some(Reason {
                    kind: "connect_failed",
                    status: None,
                    retry_after: None,
                });
            }
            if error.is_timeout() || error.is_body() || error.is_request() || error.is_decode() {
                return Some(Reason {
                    kind: "disconnected",
                    status: None,
                    retry_after: None,
                });
            }
        }
    }
    None
}

/// `Retry-After-Ms` (fractional milliseconds, sent by some OpenAI-style
/// providers) or `Retry-After` in whole seconds. HTTP dates are ignored and
/// the computed backoff is used instead.
pub fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let read = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
    };
    read("retry-after-ms")
        .map(|ms| Duration::from_secs_f64(ms / 1000.0))
        .or_else(|| read("retry-after").map(Duration::from_secs_f64))
}

/// A number in `[0, 1)` for jitter; not cryptographic.
fn unit_random() -> f64 {
    (uuid::Uuid::new_v4().as_u128() >> 75) as f64 / (1u64 << 53) as f64
}

/// Exponential backoff with "equal jitter": half the step is fixed and half is
/// random, so parallel clients spread out but never retry immediately.
pub fn backoff(attempt: u32, base_sec: f64, jitter: f64) -> Duration {
    let step = (base_sec.max(0.0) * 2_f64.powi(attempt.saturating_sub(1).min(16) as i32))
        .min(MAX_BACKOFF_SEC);
    Duration::from_secs_f64(step * (0.5 + 0.5 * jitter.clamp(0.0, 1.0)))
}

/// What to do after a failed attempt.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub reason: Reason,
    pub attempt: u32,
    pub max_attempts: u32,
    pub delay: Duration,
}
impl Plan {
    /// Payload for the `model.retry` event.
    pub fn event(&self, discard_message_id: Option<&str>) -> Value {
        json!({
            "attempt": self.attempt,
            "max_attempts": self.max_attempts,
            "reason": self.reason.kind,
            "status": self.reason.status,
            "delay_ms": self.delay.as_millis() as u64,
            "retry_after": self.reason.retry_after.is_some(),
            "discard_message_id": discard_message_id,
        })
    }
}

/// Decide whether to retry after `failed` earlier retries. `max_retries` is
/// `agent.model_retries`; `base_sec` is `agent.retry_backoff_sec`.
pub fn plan(error: &anyhow::Error, failed: u32, max_retries: u32, base_sec: f64) -> Option<Plan> {
    if failed >= max_retries {
        return None;
    }
    let reason = classify(error)?;
    let attempt = failed + 1;
    let delay = match reason.retry_after {
        // A provider asking for a longer pause than we are willing to wait
        // has effectively refused; report the error instead of hanging.
        Some(wait) if wait > MAX_RETRY_AFTER => return None,
        Some(wait) => wait,
        None => backoff(attempt, base_sec, unit_random()),
    };
    Some(Plan {
        reason,
        attempt,
        max_attempts: max_retries,
        delay,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(status: u16, retry_after: Option<u64>, local: bool) -> anyhow::Error {
        anyhow::Error::new(ModelFailure::Http {
            status,
            retry_after: retry_after.map(Duration::from_secs),
            local,
            message: format!("Model provider returned HTTP {status}"),
        })
    }

    #[test]
    fn classifies_passing_failures_only() {
        assert_eq!(
            classify(&http(429, None, false)).unwrap().kind,
            "rate_limited"
        );
        assert_eq!(
            classify(&http(529, None, false)).unwrap().kind,
            "overloaded"
        );
        assert_eq!(
            classify(&http(502, None, false)).unwrap().kind,
            "server_error"
        );
        for status in [400, 401, 403, 404, 413, 422, 501] {
            assert!(classify(&http(status, None, false)).is_none(), "{status}");
        }
        // A local runtime's 500 is usually a full context: do not retry it.
        assert!(classify(&http(500, None, true)).is_none());
        assert!(classify(&http(503, None, true)).is_some());
        let stream = anyhow::Error::new(ModelFailure::Stream {
            code: None,
            detail: "Provider returned error: Overloaded".into(),
            message: "Provider reported an error while generating".into(),
        });
        assert_eq!(classify(&stream).unwrap().kind, "stream_error");
        let invalid = anyhow::Error::new(ModelFailure::Stream {
            code: Some(400),
            detail: "invalid tool schema".into(),
            message: "Provider reported an error while generating".into(),
        });
        assert!(classify(&invalid).is_none());
        let dropped = anyhow::Error::new(ModelFailure::Disconnected {
            message: "gone".into(),
        })
        .context("Model request failed");
        assert_eq!(classify(&dropped).unwrap().kind, "disconnected");
        assert!(classify(&anyhow::anyhow!("Model request cancelled")).is_none());
    }

    #[test]
    fn backoff_grows_with_jitter_and_retry_after_wins() {
        assert_eq!(backoff(1, 1.0, 0.0), Duration::from_millis(500));
        assert_eq!(backoff(1, 1.0, 1.0), Duration::from_secs(1));
        assert_eq!(backoff(3, 1.0, 1.0), Duration::from_secs(4));
        assert_eq!(backoff(12, 1.0, 1.0), Duration::from_secs(30), "capped");
        for _ in 0..50 {
            let wait = backoff(2, 1.0, unit_random());
            assert!(wait >= Duration::from_secs(1) && wait <= Duration::from_secs(2));
        }
        let plan = plan(&http(429, Some(7), false), 0, 3, 1.0).unwrap();
        assert_eq!(plan.delay, Duration::from_secs(7));
        assert_eq!(plan.attempt, 1);
        assert_eq!(plan.event(Some("m1"))["discard_message_id"], "m1");
        assert!(super::plan(&http(429, Some(600), false), 0, 3, 1.0).is_none());
        assert!(super::plan(&http(429, None, false), 3, 3, 1.0).is_none());
        assert!(super::plan(&http(429, None, false), 0, 0, 1.0).is_none());
    }

    #[test]
    fn reads_retry_after_headers() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "3".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(3)));
        headers.insert("retry-after-ms", "250".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(Duration::from_millis(250)));
        let mut date = reqwest::header::HeaderMap::new();
        date.insert(
            "retry-after",
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&date), None);
    }
}

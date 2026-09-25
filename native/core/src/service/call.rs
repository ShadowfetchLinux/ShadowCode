//! One parsed application command (`Call`) and the lenient field type route
//! modules use for typed request bodies.
//!
//! Request bodies were always read leniently (`body["key"].as_str()`): a
//! missing field, `null`, or a value of another type reads as absent rather
//! than failing the request. `Loose<T>` keeps that contract for typed
//! `#[derive(Deserialize)]` bodies, so moving a handler to a struct never
//! changes which requests it accepts.
use super::Request;
use anyhow::{ensure, Result};
use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use serde_json::Value;
use std::collections::HashMap;

pub(crate) struct Call {
    pub method: String,
    /// The URL path without its query string (`/api/sessions/<id>`).
    pub path: String,
    /// `path` split on `/` after trimming slashes: `["api", "sessions", …]`.
    segments: Vec<String>,
    pub query: HashMap<String, String>,
    pub body: Value,
}

impl Call {
    pub(super) fn parse(request: Request) -> Result<Self> {
        ensure!(
            request.path.starts_with("/api/") && request.path.len() <= 16000,
            "Invalid application command path"
        );
        ensure!(
            request.body.to_string().len() <= 8_000_000,
            "Request exceeds 8 MB"
        );
        let url = reqwest::Url::parse(&format!("http://ipc.local{}", request.path))?;
        let query = url.query_pairs().into_owned().collect();
        let path = url.path().to_owned();
        let segments = path
            .trim_matches('/')
            .split('/')
            .map(str::to_owned)
            .collect();
        Ok(Self {
            method: request.method,
            path,
            segments,
            query,
            body: request.body,
        })
    }
    /// Path segments as borrowed strings, for slice patterns.
    pub fn parts(&self) -> Vec<&str> {
        self.segments.iter().map(String::as_str).collect()
    }
    /// The route family: the segment after `/api/`.
    pub fn family(&self) -> &str {
        self.segments.get(1).map(String::as_str).unwrap_or("")
    }
    /// A string body field; anything else reads as "".
    pub fn text(&self, key: &str) -> &str {
        self.body[key].as_str().unwrap_or("")
    }
    /// A query parameter, or "".
    pub fn q(&self, key: &str) -> &str {
        self.query.get(key).map(String::as_str).unwrap_or("")
    }
    /// `?limit=`, clamped to `1..=max`.
    pub fn limit(&self, default: usize, max: usize) -> usize {
        super::query_limit(&self.query, default, max)
    }
    /// The typed body. A body that is not a JSON object (for example the
    /// `null` of a GET) reads as the default.
    pub fn body<T: DeserializeOwned + Default>(&self) -> Result<T> {
        if self.body.is_object() {
            Ok(T::deserialize(&self.body)?)
        } else {
            Ok(T::default())
        }
    }
    /// The error every unknown route returns.
    pub fn unavailable(&self) -> anyhow::Error {
        anyhow::anyhow!(
            "Application command is not available: {} {}",
            self.method,
            self.path
        )
    }
}

/// A body field read the way `body[key].as_str()` / `.as_bool()` /
/// `.as_u64()` read it: a value of another type is treated as absent.
#[derive(Clone, Debug, Default)]
pub(crate) struct Loose<T>(pub Option<T>);

impl<'de, T: DeserializeOwned> Deserialize<'de> for Loose<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Ok(Self(serde_json::from_value(value).ok()))
    }
}

/// A string field; absent reads as "".
pub(crate) type Text = Loose<String>;
/// A boolean field; absent reads as `None` (most routes treat it as false).
pub(crate) type Flag = Loose<bool>;

impl Loose<String> {
    pub fn as_str(&self) -> &str {
        self.0.as_deref().unwrap_or("")
    }
    pub fn is_empty(&self) -> bool {
        self.as_str().is_empty()
    }
    /// The value when it is a non-empty string.
    pub fn non_empty(&self) -> Option<&str> {
        self.0.as_deref().filter(|s| !s.is_empty())
    }
}

impl Loose<bool> {
    pub fn is_true(&self) -> bool {
        self.0 == Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Body {
        name: Text,
        run: Flag,
        timeout: Loose<u64>,
    }

    #[test]
    fn loose_fields_read_like_untyped_access() {
        let call = |body: Value| Call {
            method: "POST".into(),
            path: "/api/x".into(),
            segments: vec!["api".into(), "x".into()],
            query: HashMap::new(),
            body,
        };
        let typed: Body = call(json!({"name": 3, "run": "yes", "timeout": -1}))
            .body()
            .unwrap();
        assert_eq!(typed.name.as_str(), "");
        assert!(!typed.run.is_true());
        assert_eq!(typed.timeout.0, None);
        let typed: Body = call(json!({"name": "a", "run": true, "timeout": 5}))
            .body()
            .unwrap();
        assert_eq!(typed.name.as_str(), "a");
        assert!(typed.run.is_true());
        assert_eq!(typed.timeout.0, Some(5));
        // GET requests carry a null body.
        let typed: Body = call(Value::Null).body().unwrap();
        assert!(typed.name.is_empty() && typed.run.0.is_none());
    }
}

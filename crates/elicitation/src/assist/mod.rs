//! Provider-neutral "assist" surface: shared types for the optional LLM assist
//! backends. This module is ALWAYS compiled; the concrete Jev backend lives
//! behind the `jev` cargo feature (see [`jev`]).
//!
//! The assist layer is **advisory**: it never appends events to a session log.
//! Every tool returns a proposal the caller may append itself.

#[cfg(feature = "jev")]
pub mod dispatch;
#[cfg(feature = "jev")]
pub mod jev;

use serde::Serialize;

/// Why an assist backend could not answer. Serialized `snake_case`; the two
/// HTTP classes get explicit `http_4xx` / `http_5xx` spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    Disabled,
    NoKey,
    Timeout,
    #[serde(rename = "http_4xx")]
    Http4xx,
    #[serde(rename = "http_5xx")]
    Http5xx,
    Network,
    BadResponse,
    BadConfig,
    FeatureNotBuilt,
}

impl UnavailableReason {
    /// Stable `snake_case` label used in the MCP error message.
    pub fn as_str(self) -> &'static str {
        match self {
            UnavailableReason::Disabled => "disabled",
            UnavailableReason::NoKey => "no_key",
            UnavailableReason::Timeout => "timeout",
            UnavailableReason::Http4xx => "http_4xx",
            UnavailableReason::Http5xx => "http_5xx",
            UnavailableReason::Network => "network",
            UnavailableReason::BadResponse => "bad_response",
            UnavailableReason::BadConfig => "bad_config",
            UnavailableReason::FeatureNotBuilt => "feature_not_built",
        }
    }
}

/// A provider-neutral "assist is not available" result. `detail` is bounded
/// and must never carry a credential (callers scrub before constructing).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssistUnavailable {
    pub reason: UnavailableReason,
    pub detail: String,
}

/// Maximum length (in `char`s) of an [`AssistUnavailable::detail`].
const MAX_DETAIL_CHARS: usize = 300;

impl AssistUnavailable {
    /// Builds an unavailable result, truncating `detail` to the bounded
    /// length on a `char` boundary.
    pub fn new(reason: UnavailableReason, detail: impl Into<String>) -> Self {
        let detail = detail.into().chars().take(MAX_DETAIL_CHARS).collect();
        Self { reason, detail }
    }

    /// Renders the wire shape used by non-MCP callers:
    /// `{"ok": false, "status": "assist_unavailable", ...}`.
    pub fn to_value(&self, links: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "ok": false,
            "status": "assist_unavailable",
            "reason": self.reason,
            "detail": self.detail,
            "links": links,
        })
    }

    /// Map to a stable MCP error whose message begins `ASSIST_UNAVAILABLE`.
    pub fn to_mcp(&self) -> rmcp::ErrorData {
        rmcp::ErrorData::internal_error(
            format!(
                "ASSIST_UNAVAILABLE: {}: {}",
                self.reason.as_str(),
                self.detail
            ),
            None,
        )
    }
}

/// Replaces every occurrence of `secret` with `[redacted]`. An empty secret
/// leaves the text untouched (it would otherwise match everywhere).
pub fn scrub(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_string();
    }
    text.replace(secret, "[redacted]")
}

/// Provider-neutral token accounting for one assist call.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
}

/// The assist opt-out. `ELICITATION_ASSIST` is a comma list; supplying a paid
/// key is the opt-in. An unset or blank list leaves assist enabled; an
/// explicitly non-matching list (e.g. `rig`) disables it. The wildcard `all`
/// and sentinel `1` also enable the backend.
pub fn assist_enabled(lookup: &dyn Fn(&str) -> Option<String>, feature: &str) -> bool {
    match lookup("ELICITATION_ASSIST") {
        None => true,
        Some(raw) if raw.trim().is_empty() => true,
        Some(raw) => raw
            .split(',')
            .map(str::trim)
            .any(|token| !token.is_empty() && (token == feature || token == "all" || token == "1")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_lookup(value: &str) -> impl Fn(&str) -> Option<String> + '_ {
        move |_| Some(value.to_string())
    }

    fn absent_lookup() -> impl Fn(&str) -> Option<String> {
        |_| None
    }

    #[test]
    fn assist_enabled_recognizes_jev_in_comma_list() {
        assert!(assist_enabled(&fixed_lookup("jev,rig"), "jev"));
    }

    #[test]
    fn assist_enabled_defaults_on_when_toggle_unset() {
        assert!(assist_enabled(&absent_lookup(), "jev"));
    }

    #[test]
    fn assist_enabled_honours_an_explicit_opt_out() {
        assert!(!assist_enabled(&fixed_lookup("rig"), "jev"));
    }

    #[test]
    fn to_value_reports_assist_unavailable_status() {
        let unavailable = AssistUnavailable::new(UnavailableReason::NoKey, "no key");
        let value = unavailable.to_value(&[]);
        assert_eq!(value["status"], "assist_unavailable");
    }
}

//! Session-configurable coverage schema (spec §6).
//!
//! Readiness requires every *required* coverage dimension to carry a
//! `withstood`+ hypothesis. The schema is session-configurable; the default is
//! the five canonical discovery dimensions.

use serde::{Deserialize, Serialize};

/// One coverage dimension the interview must satisfy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageDimension {
    /// The dimension tag, matched by exact equality against `Claim.dimensions`.
    pub tag: String,
    /// Whether this dimension must be covered for readiness (spec §6.1).
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

impl CoverageDimension {
    /// A required dimension with the given tag.
    pub fn required(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            required: true,
        }
    }
}

/// The session's coverage schema (spec §6). Default: the five canonical
/// discovery dimensions, all required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageSchema {
    pub dimensions: Vec<CoverageDimension>,
}

impl Default for CoverageSchema {
    fn default() -> Self {
        Self {
            dimensions: [
                "purpose",
                "constraints",
                "success-criteria",
                "scope",
                "non-goals",
            ]
            .iter()
            .map(|t| CoverageDimension::required(*t))
            .collect(),
        }
    }
}

impl CoverageSchema {
    /// The tags of all required dimensions, in schema order.
    pub fn required_tags(&self) -> impl Iterator<Item = &str> {
        self.dimensions
            .iter()
            .filter(|d| d.required)
            .map(|d| d.tag.as_str())
    }
}

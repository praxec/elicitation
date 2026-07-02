//! The future research backend seam (spec §1 / §7).
//!
//! In v1 this is a `None` seam: the server **never** researches the app itself
//! (spec §1, hard boundary 2). App-research evidence enters only as caller-
//! supplied `app_research_*` events. [`ResearchProvider`] is reserved for a
//! future online backend and is intentionally unused by the kernel.

/// A future online research backend (Perplexity/etc.). Deferred — `None` in v1.
pub trait ResearchProvider: Send + Sync {
    /// Run a research query. Reserved for a future revision.
    fn research(&self, query: &str) -> Option<String>;
}

/// The v1 provider: researches nothing (spec §1 hard boundary).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoResearch;

impl ResearchProvider for NoResearch {
    fn research(&self, _query: &str) -> Option<String> {
        None
    }
}

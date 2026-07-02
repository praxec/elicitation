//! Shared test builders for the elicitation-core golden/table tests.

use std::sync::Arc;

use elicitation_core::{
    Category, Claim, Engine, EntityRef, EvidenceRef, FilesystemStore, Modality, Object, Polarity,
};

/// An engine backed by a fresh temp dir. The `TempDir` is returned so the test
/// keeps it alive for the duration of the test.
pub fn temp_engine() -> (Engine, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FilesystemStore::new(dir.path()).expect("store");
    (Engine::new(Arc::new(store)), dir)
}

/// Build a positive claim with the given identity. Tweak via the returned value.
pub fn claim(session: &str, id: &str, subject: &str, predicate: &str, object: &str) -> Claim {
    Claim {
        id: id.to_string(),
        session_id: session.to_string(),
        subject: EntityRef::new(subject),
        predicate: predicate.to_string(),
        object: Object::Value {
            value: object.to_string(),
        },
        polarity: Polarity::Positive,
        category: Category::Context,
        modality: Modality::Fact,
        dimensions: Vec::new(),
        source: EvidenceRef::new("turn-1"),
        time_scope: None,
        condition: None,
    }
}

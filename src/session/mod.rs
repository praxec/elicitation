// Session core module — state machine + schema + store + durable layer.
// This file is the module root wired into src/lib.rs.

pub mod machine;
pub mod recovery;
pub mod registry;
pub mod schema;
pub mod sqlite_store;
pub mod state;
pub mod store;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod contract_tests;

pub use machine::{append_answer, append_questions, confirm, request_confirm, SmError};
pub use schema::{NonEmptyPrompt, Question, QuestionId, QuestionKind};
pub use state::{Answer, SessionState, SessionStatus, SingleUseToken};
pub use store::{MemorySessionStore, SessionStore, StoreError};
pub use sqlite_store::SqliteSessionStore;
pub use recovery::{RecoveryEngine, RecoveryReport};
pub use registry::{SessionRegistry, RegistryError};

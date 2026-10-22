//! dot — the library behind the tod CLI.
//!
//! A Rust port of the "dot" library of
//! [tod-cli](https://github.com/timber-dev-society/tod-cli): a tiny
//! schema-based document format ("Key: value" lines, tab-indented
//! multi-line values) used to store one todo per plain-text file,
//! plus the git-native todo store and the project planning tools
//! built on top of it.
//!
//! ## Storage
//!
//! The todos live in a dedicated `tod` branch: one directory per
//! state (backlog, todo, review, test, done, canceled), one
//! subdirectory per task named by its hash, holding `task.tod`
//! (the task itself), `plan.md` (its micro-todo checklist),
//! `spec.md` (the agent-facing work description) and optionally
//! `scenario.feature` (its Gherkin acceptance criteria).

pub mod brief;
pub mod git;
pub mod plan;
pub mod scenario;
pub mod schema;
pub mod store;
pub mod task;
pub mod types;

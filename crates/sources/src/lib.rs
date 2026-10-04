//! `Source` implementations for `tasq` (plan Phase 5): a shared forge client
//! for GitLab and GitHub, the review-request and work-item sources built on
//! it, and the LLM bridge that turns a command's JSON into items.
//!
//! All HTTP goes through [`http::Transport`], an injected trait, so every
//! client is tested against scripted responses and `cargo test` never
//! touches the network (FR-12). [`http::UreqTransport`] is the real one.

#![warn(missing_docs)]

pub mod auth;
pub mod forge;
pub mod github;
pub mod gitlab;
pub mod http;
pub mod url;

pub use self::forge::{Forge, MergeRequest, MrState, User, WorkItem};
pub use self::http::{Client, HttpError, HttpResponse, Transport, UreqTransport};
pub use self::url::{ForgeRef, RefKind, parse_ref};

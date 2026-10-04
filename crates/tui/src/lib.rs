//! The ratatui terminal UI of `tasq` (plan Phase 8), the second consumer
//! of `tasq-core` and the proof of the core/UI boundary (ADR-0005).
//!
//! The crate depends on `tasq-core` and ratatui only. Everything else is
//! injected: the [`Store`](tasq_core::store::Store) the tasks come from,
//! the [`Clock`](tasq_core::clock::Clock) that stamps notes and a [`Host`]
//! for what needs the outside world (an editor, a work session, `sync`),
//! which the CLI provides by running itself.
//!
//! The architecture is Elm's: a [`Model`], a [`Msg`] type, a pure
//! [`update()`] that returns [`Cmd`]s, a [`view()`] that draws the model, and a
//! runtime ([`run`]) that reads the terminal, runs the commands
//! ([`dispatch`]) and feeds their results back as messages. Every edit the
//! UI makes is one of the `tasq_core::edit` functions the CLI uses (FR-10).

#![warn(missing_docs)]

pub mod keys;
pub mod model;
pub mod msg;
pub mod runtime;
pub mod update;
pub mod view;

pub use model::{Mode, Model, NoteTarget};
pub use msg::{Cmd, Host, HostResult, Msg, NoHost, RecordingHost};
pub use runtime::{dispatch, run};
pub use update::update;
pub use view::view;

//! One module per command. Each `run` is a core or store call plus
//! rendering, in both human and `--json` form.

pub mod completions;
pub mod config;
pub mod create;
pub mod doctor;
pub mod edit;
pub mod list;
pub mod mr;
pub mod store;

//! One module per command. Each `run` is a core or store call plus
//! rendering, in both human and `--json` form.

pub mod completions;
pub mod config;
pub mod doctor;
pub mod list;
pub mod store;

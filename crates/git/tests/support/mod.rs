//! Helpers shared by the integration tests of `tasq-git`.

use std::process::Command;

/// Gives `cmd` this process's `LLVM_PROFILE_FILE`, when it has one. Under
/// `cargo llvm-cov` the binaries are instrumented and name their profile
/// by that variable; a child started with `env_clear` and without it
/// writes `default_*.profraw` relative to its working directory, which
/// fails on stderr when a test starts it in a directory it removed.
pub fn forward_profile(cmd: &mut Command) -> &mut Command {
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", profile);
    }
    cmd
}

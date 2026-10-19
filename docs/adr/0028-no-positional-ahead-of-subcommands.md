# ADR-0028: No positional argument ahead of subcommands

- **Status:** Accepted
- **Date:** 2026-10-19

## Context

Two commands took a positional argument in the same slot as their subcommands:

1. `tasq [WORD]`, the original script's `tasks <status|tag>` shorthand for
   `tasq list <WORD>`.
2. `tasq worktree <ID> [BRANCH|PATH]` (declare or track) next to `tasq worktree up`,
   `release`, `ls`, ... (ADR-0022), parsed with `args_conflicts_with_subcommands`.

clap parses both, but `clap_complete` cannot express them. Its zsh script puts the
positional spec ahead of the subcommand slot, so the first word after `tasq` was
completed as `WORD` and `tasq worktree <TAB>` offered the top-level commands again;
after `tasq worktree` the first word was completed as `<ID>` (files), so
`tasq worktree rel<TAB>` never reached `release`. Every nested subcommand
(`store`, `config`, `plugins`, `worktree`) was affected.

The bare word was also the one place where a plugin and a built-in behaviour
competed (ADR-0006: "`tasq <word>` is ambiguous between a plugin and a filter").

## Decision

No command takes a positional argument in the slot of its subcommands.

- The list filter is `tasq list [WORD]` only, with `ls` as a visible alias. A bare
  `tasq` still lists open tasks; `tasq ready` is now an unknown command (exit 2)
  unless a `tasq-ready` plugin exists.
- Declaring or tracking a worktree is `tasq worktree set <ID> [BRANCH|PATH]`
  (hidden `--create <BRANCH>` moves with it). A bare `tasq worktree` prints the
  subcommand list.

A unit test checks the generated zsh script: no `_arguments` call has a
positional spec ahead of its subcommand slot.

## Consequences

### Positive

- Shell completion of every subcommand works with the stock `clap_complete`
  generators, for zsh, bash and fish.
- The plugin rule loses its only ambiguity: a word that is not a built-in or an
  alias is a plugin name or an error, never a filter.
- The parser loses `args_conflicts_with_subcommands`, `subcommand_negates_reqs`
  and the "unexpected argument before the command" check.

### Negative

- Breaking for muscle memory and scripts: `tasq ready` and `tasq worktree 3 b`
  fail. Mitigated by the `ls` alias (`tasq ls ready`) and by clap's error naming
  the unknown subcommand. No release exists yet (version `0.1.0`).
- The session prompt and the `/tasq:wrapup` skill had to change; sessions started
  from an older prompt that run `tasq worktree <ID> <PATH>` get a usage error.

## Alternatives considered

- **Keep both forms and hide the positionals from completion only.** Works for zsh
  (the generator lists hidden arguments, so they would have to be dropped from the
  completion command), but leaves two grammars to document and the plugin
  ambiguity in place.
- **Dynamic completion (`clap_complete`'s `unstable-dynamic`).** Would model the
  grammar at TAB time, but the feature is unstable and changes how completions are
  installed; it does not remove the ambiguity either.

## References

- ADR-0006 (plugin mechanism), ADR-0022 (worktrees).
- `crates/cli/src/cli.rs` (`Command::List`, `WorktreeAction::Set`),
  `crates/cli/src/commands/completions.rs` (`subcommands_complete_as_the_first_word`).

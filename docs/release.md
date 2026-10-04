# Releasing tasq

**Status: the pipeline has not run yet.** The GitHub repository does not exist;
`OWNER` is a placeholder in `Cargo.toml`, `cliff.toml`, `README.md` and the
workflow comments. The first tag will be the first real run.

The release pipeline is a hand-written GitHub Actions workflow,
[`.github/workflows/release.yml`](../.github/workflows/release.yml).
`cargo-dist` and `release-plz` were considered and not used: both generate
workflow code that has to be regenerated whenever the tool is upgraded, neither
is installed on the author's machine, and the workflow needed here is small
(four native builds, one release, six `cargo publish` calls, one formula).
Release notes come from the conventional commits through
[git-cliff](https://git-cliff.org) and [`cliff.toml`](../cliff.toml).

## Cutting a release

1. Make sure `main` is green and the working tree is clean.
2. Regenerate `CHANGELOG.md` for the version you are about to tag. Either
   rewrite the whole file:

   ```sh
   git cliff --tag vX.Y.Z -o CHANGELOG.md
   ```

   or prepend only the new section to the hand-written history:

   ```sh
   git cliff --unreleased --tag vX.Y.Z --prepend CHANGELOG.md
   ```

   Read the result. Commits that are not conventional are dropped
   (`filter_unconventional = true`); `test` and `chore(release)` commits are
   skipped on purpose.
3. Bump `version` in the root `Cargo.toml` (`[workspace.package]`; every crate
   inherits it). Do not touch the `version = "0.1.0"` on the path dependencies
   in `crates/*/Cargo.toml` separately: they must equal the workspace version,
   so bump them in the same commit.
4. Commit and tag:

   ```sh
   git commit -am "chore(release): vX.Y.Z"
   git tag vX.Y.Z
   git push origin main vX.Y.Z
   ```

The `check-version` job fails the run if the tag does not equal the version in
`Cargo.toml`, so a mistyped tag costs one build minute, not a half release.
`cargo publish` is the step that cannot be undone (a published version can be
yanked, never replaced), so look at the dry run below before the first tag.

## What the workflow does

| Job | Runs on | Does |
|---|---|---|
| `check-version` | ubuntu | Reads `version` from `Cargo.toml`; on a tag, fails unless `vVERSION == tag`. Exposes `version` to the other jobs. |
| `build` (matrix) | `ubuntu-latest`, `ubuntu-24.04-arm`, `macos-13`, `macos-latest` | Checks the runner's host triple is the matrix target (no cross-compilation), `cargo build --release --locked -p tasq`, `tasq --version`, then packs `tasq-vVERSION-TARGET.tar.gz` (`tasq`, `README.md`, `LICENSE`, `CHANGELOG.md`) plus a `.sha256`. Uploads both as an artifact. |
| `release` | ubuntu | Downloads every artifact, renders the notes for the tag with `git cliff --latest --strip header`, creates the GitHub release (`softprops/action-gh-release@v2`) with every tarball and checksum attached. The only job with `contents: write`. A tag containing `-` (`v0.2.0-rc.1`) is marked pre-release. |
| `publish` | ubuntu | `cargo publish --locked -p <crate>` in dependency order: `tasq-core`; `tasq-store-nb`, `tasq-sources`, `tasq-launch`, `tasq-tui`; `tasq`. Each publish is skipped when that version already exists on crates.io, and is followed by a poll of the sparse index until the version is visible. Needs `CARGO_REGISTRY_TOKEN`. |
| `homebrew` | ubuntu | Only when `HOMEBREW_TAP_TOKEN` is set. Renders `homebrew/tasq.rb.template` with the four tarball URLs and checksums and pushes it to `<owner>/homebrew-tasq` as `Formula/tasq.rb`. |

Targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-apple-darwin`, `aarch64-apple-darwin`. No Windows (plan Non-Goals).

`publish` and `homebrew` depend on `release`, so nothing is published unless
the binaries built and the GitHub release exists.

## Secrets

| Secret | Needed by | Notes |
|---|---|---|
| `CARGO_REGISTRY_TOKEN` | `publish` | A crates.io API token with `publish-new` and `publish-update` scopes for the six crates. The job fails early when it is missing. |
| `HOMEBREW_TAP_TOKEN` | `homebrew` | Optional. A fine-grained token with `contents: write` on the `<owner>/homebrew-tasq` repository. When absent the job logs one line and does nothing. |

`GITHUB_TOKEN` is provided by Actions and is enough for the release itself.

## First publish

- The crate names `tasq`, `tasq-core`, `tasq-store-nb`, `tasq-sources`,
  `tasq-launch` and `tasq-tui` must be free on crates.io, or owned by the token's
  account. Check before tagging: `https://crates.io/crates/tasq`. If `tasq` is
  taken, the binary crate has to be renamed first (the plan notes that renaming
  is a find-and-replace on crate names).
- Order matters. `tasq-store-nb` declares `tasq-core = { path = "../core",
  version = "0.1.0" }`; `cargo publish` strips the `path` and the verification
  build resolves `tasq-core 0.1.0` from the registry index. That is why the job
  waits for each crate to appear in `https://index.crates.io/` before
  publishing the next one. Recent cargo versions also wait on their own after
  an upload; the explicit poll is belt and braces, with a 10 minute cap.
- crates.io has no README for these crates: `readme` is not set because the
  only README lives at the workspace root, and `cargo publish` only packages
  files inside the crate directory. `tasq-launch` ships its prompt templates
  because they sit in `crates/launch/templates/` and are pulled in with
  `include_str!`.
- A failed run can be re-run from the Actions page. Published crates are
  skipped, an existing GitHub release is updated in place by
  `action-gh-release`, and the formula push is a no-op when the file is
  unchanged.

## Dry run

Run the workflow by hand from the Actions tab (`workflow_dispatch`). It builds
and packages every target, renders the notes for the unreleased commits and
uploads them as the `release-notes` artifact. It creates no release, publishes
nothing and never touches the tap. Download the artifacts to check the tarball
layout and the checksums before tagging.

## Verify

After the run:

- [ ] The release page lists eight files (four tarballs, four `.sha256`).
- [ ] `curl -LO <tarball>` and `shasum -a 256 -c <tarball>.sha256` passes.
- [ ] `tar xzf <tarball> && ./tasq-vX.Y.Z-<target>/tasq --version` prints `tasq X.Y.Z`.
- [ ] `cargo install tasq` (on a machine without the checkout) installs `X.Y.Z`
      and `tasq --version` agrees.
- [ ] With the tap: `brew tap <owner>/tasq && brew install tasq && brew test tasq`.
- [ ] The notes on the release page match the new `CHANGELOG.md` section.

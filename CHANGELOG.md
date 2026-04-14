# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Removed

- Unused `backend` / `PKGREP_BACKEND` configuration handling after the object storage feature removal.
- Unused `.dev/azurite` and `.dev/seaweedfs` object-store fixture files left over from the deleted remote-cache test flow.

### Changed

- Simplified `pkgrep pull` completion output now that all pulls resolve through the local git cache path.

## [0.7.0] - 2026-04-14

### Removed

- Object storage remote cache support (`s3`, `azure_blob`), including the `cache hydrate` command and remote cache E2E test workflow.

## [0.6.0] - 2026-04-13

### Added

- `pkgrep pull --fallback-repo-head` to explicitly clone a dependency repository's default branch when package metadata cannot be mapped to an exact upstream git revision.

### Changed

- `pkgrep pull` now fails with a targeted retry hint when npm metadata resolves to a repository URL but does not provide a deterministic source revision, including the exact `--fallback-repo-head` command to retry.

## [0.5.1] - 2026-03-30

### Changed

- Bundled `pkgrep-usage` guidance now treats linked `.pkgrep` dependency paths as internal details and tells agents to report dependency findings with inline snippets or summaries instead of opaque local path references.
- `pkgrep init` now adds the same user-facing guidance to generated `AGENTS.md` instructions.
- README clarifies that `pkgrep skill install --force` replaces an existing install with the latest bundled skill copy.

### Dependencies

- Updated `clap`, `tracing-subscriber`, `tar`, and `testcontainers`.
- Updated release workflow actions `Swatinem/rust-cache` and `softprops/action-gh-release`.

## [0.5.0] - 2026-03-13

### Added

- `pkgrep list` with optional `--json` output for project-linked dependency inspection.
- `pkgrep init` for opt-in project setup of `.pkgrep/`, `AGENTS.md`, and the bundled skill.
- Provider/project detection for `pnpm-lock.yaml`, `yarn.lock`, `uv.lock`, and `Cargo.lock`.
- `crates:` dependency support, including crates.io resolution and `Cargo.lock` parsing.

### Changed

- Bare git dependency specs now resolve against the remote default branch automatically.
- Versionless loose pulls now prefer locally detected versions from npm, PyPI (`uv.lock`), and crates (`Cargo.lock`) before falling back to registry latest.
- Shorthand package inference now covers npm, PyPI, and crates ecosystems when a single supported project lockfile context is present.

### Dependencies

- Updated key tooling/dependencies including `tokio`, `toml`, `tempfile`, and `assert_cmd`, plus the `actions/download-artifact` release workflow action.

## [0.4.0] - 2026-03-05

### 🚀 Features

- *(path)* Support npm/pypi lookups via manifest metadata (#13)

## [0.3.1] - 2026-02-24

### Fixed

- `pkgrep self update` now exits without replacing the executable when the installed binary already matches the latest release asset bytes.

## [0.3.0] - 2026-02-24

### Added

- `pkgrep self update` command to update the current binary from GitHub Releases with SHA256 verification.

### Changed

- `pkgrep self update` now detects Homebrew-managed installs and directs users to `brew upgrade pkgrep`.

## [0.2.0] - 2026-02-24

### Added

- `pkgrep skill install` command with non-interactive install semantics.
- Bundled `pkgrep-usage` Agent Skills-compatible skill under `skills/pkgrep-usage`.

### Changed

- Skill install defaults now use `.agents/skills`:
  - project scope: `<cwd>/.agents/skills`
  - global scope: `$HOME/.agents/skills`
- README updated to document CLI-first skill installation workflow.

## [0.1.0] - 2026-02-24

### Added

- Initial `pkgrep` release.
- Core commands:
  - `pkgrep pull`
  - `pkgrep path`
  - `pkgrep remove --yes`
  - `pkgrep cache hydrate`
  - `pkgrep cache clean --yes`
  - `pkgrep cache prune [--yes]`
- Central cache model with project symlink links and local index files for dependency references.
- Git-only source retrieval pipeline with npm/PyPI dependency resolution mapped to upstream git repos.
- Remote cache support via object stores (`s3`, `azure_blob`) with hydrate/publish flow.

[Unreleased]: https://github.com/thomasjiangcy/pkgrep/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/thomasjiangcy/pkgrep/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/thomasjiangcy/pkgrep/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/thomasjiangcy/pkgrep/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/thomasjiangcy/pkgrep/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/thomasjiangcy/pkgrep/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/thomasjiangcy/pkgrep/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/thomasjiangcy/pkgrep/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/thomasjiangcy/pkgrep/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/thomasjiangcy/pkgrep/releases/tag/v0.1.0

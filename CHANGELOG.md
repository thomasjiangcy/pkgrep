# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/thomasjiangcy/pkgrep/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/thomasjiangcy/pkgrep/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/thomasjiangcy/pkgrep/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/thomasjiangcy/pkgrep/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/thomasjiangcy/pkgrep/releases/tag/v0.1.0

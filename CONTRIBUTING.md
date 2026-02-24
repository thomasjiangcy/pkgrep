# Contributing to pkgrep

Thanks for contributing. This guide focuses on the fastest path to ship safe changes.

## Prerequisites

- Rust toolchain from `rust-toolchain.toml`
- `mise`
- `just`
- `lefthook`

## Setup

```bash
mise install
just hooks-install
just ci
```

## Local Workflow

```bash
# format
just fmt

# lint
just lint

# test
just test

# full local gate (required before PR)
just ci

# run local git hook checks on demand
just hooks-run
```

## Testing Policy

- Do not add mocking/stubbing frameworks or mock servers.
- Do not add mock/stub/fake test layers for provider or command behavior.
- Prefer integration tests with real inputs and real processes (fixtures, local git repos, testcontainers-backed services).
- The project enforces this with `just test-no-mocks` (included in `just ci`).

Remote cache E2E tests require Docker:

```bash
just test-remote-s3
just test-remote-azure
```

## Adding a Lockfile Provider

Use this checklist when adding support for a new project lockfile.

1. Add a provider parser module under `src/providers/`.
2. Register the provider in `src/providers/mod.rs`:
   - add `ProviderKind` variant
   - detect the lockfile in `detect_supported_project_files`
   - dispatch parse in `parse_provider_input`
3. If the provider introduces a new ecosystem, update:
   - `ProviderEcosystem` in `src/providers/mod.rs`
   - ecosystem mapping in `src/commands/pull.rs`
4. Add fixtures under `fixtures/` for the new lockfile format.
5. Add unit tests for detection and parsing in `src/providers/mod.rs` and/or the provider module.
6. Add an integration test in `tests/cli_integration.rs` proving end-to-end pull resolution from that lockfile.
7. Update `README.md` if supported lockfiles, examples, or behavior changed.
8. Run `just ci` before opening a PR.

## Provider Output Contract

Every provider must emit `NormalizedDependency` values with consistent semantics:

- `ecosystem`: package ecosystem for the dependency (`npm`, `pypi`, etc.).
- `name`: package name from the lockfile.
- `version`: resolved version string from the lockfile.
- `git_hint`: set only when the lockfile has a git source (`url` + `requested_revision`).
- `repository_url`: optional metadata; keep `None` unless reliably available.

Parsing expectations:

- Providers should be deterministic and side-effect free (no network calls).
- Skip entries that do not have enough information to form a stable dependency identity.
- Prefer entries with `git_hint` when de-duplicating.

## PR Expectations

- Keep command behavior non-interactive by default.
- Keep destructive operations explicitly gated (`--yes` pattern).
- Preserve clear, concise user-facing output for humans and coding agents.
- Include tests for behavior changes.

## Release Notes for Maintainers

- Tagging `v*` triggers binary build and GitHub Release publishing.
- Homebrew formula publish on release requires:
  - repository variable `HOMEBREW_TAP_REPOSITORY`
  - repository secret `HOMEBREW_TAP_GITHUB_TOKEN`

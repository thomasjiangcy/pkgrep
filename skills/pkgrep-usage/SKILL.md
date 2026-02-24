---
name: pkgrep-usage
description: Use pkgrep to fetch and link dependency source code for agent-assisted code traversal in JS/Python projects, including pull/path/remove/cache commands and safe prune workflows.
---

# pkgrep Usage

Use this skill when a user asks to inspect dependency source code in a project via `pkgrep`.

## Preconditions

- `pkgrep` is installed and available on `PATH`.
- Run commands from the target project directory unless the user says otherwise.
- Prefer non-interactive commands. Only pass `--yes` when the user explicitly asks to mutate/delete.

## Core Workflow

1. If dependency is known, run `pkgrep pull <dep-spec>`.
2. If dependency source path is needed, run `pkgrep path <dep-spec>`.
3. If the project is the source of truth, run `pkgrep pull` (auto lockfile detection).
4. For targeted cleanup, run `pkgrep remove <dep-spec ...> --yes`.
5. For cache cleanup, run `pkgrep cache prune` (dry-run) before `pkgrep cache prune --yes`.

## Dependency Spec Rules

- Git spec: `git:<repo-url>@<revision>` or `git:<repo-url>#<revision>`.
- npm spec: `npm:<name>` or `npm:<name>@<version>`.
- PyPI spec: `pypi:<name>` or `pypi:<name>@<version>`.
- Shorthand: `<name>` or `<name>@<version>` only when exactly one supported ecosystem is inferred in cwd.

## Safety Rules

- Do not run destructive commands without explicit user confirmation (`--yes`).
- Prefer showing dry-run output before prune.
- If shorthand inference is ambiguous, switch to explicit ecosystem specs.

For examples and troubleshooting, read `references/commands.md`.

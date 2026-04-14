# pkgrep Command Cookbook

## Basic Pulls

```bash
# Pull React source at a tag
pkgrep pull git:https://github.com/facebook/react.git@v18.3.1

# Pull npm package source
pkgrep pull npm:zod@3.23.8

# Pull PyPI package source
pkgrep pull pypi:requests@2.32.3
```

## Shorthand Pull

```bash
# Works only when exactly one ecosystem is inferred from lockfiles in cwd
pkgrep pull zod@3.23.8
```

If shorthand is ambiguous, use explicit prefix:

```bash
pkgrep pull npm:zod@3.23.8
```

## Project Lockfile Pull

```bash
# Auto-detect supported lockfiles and pull git-backed deps
pkgrep pull
```

## Resolve Linked Path

```bash
pkgrep path git:https://github.com/facebook/react.git@v18.3.1
```

Use `pkgrep path` for local inspection. In user-facing responses, prefer citing the relevant package/module/function and include the needed snippet or summary inline instead of only returning the local `.pkgrep/...` path.

## Remove Links

```bash
# No-op without --yes
pkgrep remove git:https://github.com/facebook/react.git@v18.3.1 --yes
```

## Cache Commands

```bash
# Dry-run prune first
pkgrep cache prune

# Apply prune
pkgrep cache prune --yes

# Full local cache clean (requires --yes)
pkgrep cache clean --yes
```

## Common Failure Cases

- `pkgrep pull <name>@<version>` fails with ambiguity:
  - Multiple ecosystems detected. Use `npm:` or `pypi:` prefix.
- `pkgrep path` returns no link:
  - Dependency was not pulled in this project yet, or was removed.

## Git

- Use Conventional Commits for commit messages.
- Never include unrelated changes in a commit. Only stage files that are directly relevant to the request.
- Keep `lefthook.yml` and `just` workflows aligned when changing local quality gates.

## Language Guidance

### Rust

- Do NOT use unwraps or anything that can panic in Rust code, handle errors. Obviously in tests unwraps and panics are fine!
- In Rust code I prefer using `crate::` to `super::`; please don't use `super::`. If you see a lingering `super::` from someone else clean it up.
- Avoid `pub use` on imports unless you are re-exposing a dependency so downstream consumers do not have to depend on it directly.
- Skip global state via `lazy_static!`, `Once`, or similar; prefer passing explicit context structs for any shared state.
- Prefer strong types over strings, use enums and newtypes when the domain is closed or needs validation.
- No mocking/stubbing frameworks in tests. Prefer real integration behavior via fixtures, local repos/processes, and testcontainers where needed.

## Version Currency Policy

- Always use latest stable versions of tools, dependencies, and CI actions unless explicitly constrained by compatibility requirements.
- Before introducing or updating dependencies/tools, verify latest available versions.
- Prefer moving channels like Rust `stable` where practical so new environments pick up current stable toolchains.
- If a dependency/tool cannot be updated to latest, document the reason in the related PR/commit.
- Keep project tooling definitions in `.mise.toml` aligned with the latest stable tool choices.

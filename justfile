set shell := ["zsh", "-cu"]

default:
    @just --list

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

lint:
    cargo clippy --locked --all-targets --all-features -- -D warnings

check:
    cargo check --locked --all-targets --all-features

test:
    cargo test --locked --all-targets --all-features

deps-check:
    cargo deny check

deps-unused:
    cargo machete

test-no-mocks:
    ./.dev/check_no_mocks.sh

hooks-install:
    lefthook install

hooks-run:
    lefthook run pre-commit && lefthook run pre-push

ci: fmt-check lint deps-check deps-unused test-no-mocks test

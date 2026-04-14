set shell := ["zsh", "-cu"]

default:
    @just --list

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

lint:
    cargo clippy --all-targets --all-features -- -D warnings

check:
    cargo check --all-targets --all-features

test:
    cargo test --all-targets --all-features

test-no-mocks:
    ./.dev/check_no_mocks.sh

hooks-install:
    lefthook install

hooks-run:
    lefthook run pre-commit && lefthook run pre-push

ci: fmt-check lint test-no-mocks test

set shell := ["bash", "-euc"]
set windows-shell := ["bash.exe", "-uc"]
set quiet := true

default:
    just --list

patch:
    cargo release patch --no-publish --execute

ci:
    cargo fmt --all -- --check
    cargo check --all-targets --all-features
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all-targets --all-features

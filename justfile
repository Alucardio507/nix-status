# Run `just` to list recipes. All tools come from `nix develop` (or direnv).

default:
    @just --list

# Run the CLI, e.g. `just run --json`
run *args:
    cargo run --quiet -- {{ args }}

# Fast local gate: exactly what the Rust CI job runs
check:
    cargo fmt --check
    cargo clippy --locked -- -D warnings
    cargo test --locked

# Everything CI runs (adds the Nix flake check)
ci: check
    nix flake check

# Format Rust and Nix
fmt:
    cargo fmt
    nix fmt

# Release binary size at opt-level z (current) vs s, so size changes are measured, not guessed
size:
    #!/usr/bin/env bash
    set -euo pipefail
    for level in z s; do
        CARGO_PROFILE_RELEASE_OPT_LEVEL=$level \
            cargo build --release --locked --quiet --target-dir "target/size-$level"
        printf 'opt-level=%s  %s bytes\n' "$level" "$(wc -c < "target/size-$level/release/nixos-status")"
    done

# Which crates take space in the release binary (symbols kept just for this run)
bloat:
    CARGO_PROFILE_RELEASE_STRIP=false cargo bloat --release --locked --crates -n 15

# Cut a release, e.g. `just release 0.1.4`
release version:
    ./scripts/release.sh {{ version }}

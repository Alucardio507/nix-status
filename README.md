# nixos-status

Rust CLI for [status.nixos.org](https://status.nixos.org/). It identifies your
local `nix-channel` entries, compares the running system revision with the
channel head, and prints the installed NixOS version when available.

## Install / run

```sh
nix run github:Alucardio507/nix-status            # try it
nix profile install github:Alucardio507/nix-status
```

The flake downloads the prebuilt binary listed in `release.json`. To compile
from source instead: `nix run github:Alucardio507/nix-status#source`.

## Usage

```sh
nixos-status              # summary box + table of all channels
nixos-status --json       # machine-readable output
nixos-status --no-color   # also honours NO_COLOR
```

## Development

```sh
nix develop      # or `direnv allow`: cargo, clippy, rust-analyzer, just, ...
just             # list recipes
just run --json  # run the CLI
just check       # fmt + clippy + tests (same as the Rust CI job)
just ci          # check + `nix flake check`
just size        # binary size at opt-level z vs s
just bloat       # which crates take space in the binary
```

### CI

| Workflow | Runs when these change | Does |
| --- | --- | --- |
| `ci.yml` | `src/`, `Cargo.toml`, `Cargo.lock` | fmt, clippy, tests |
| `nix.yml` | the above + `flake.nix`, `flake.lock`, `release.json` | `nix flake check` |

Docs-only changes (README, LICENSE, ...) trigger nothing.

### Releasing

```sh
just release 0.1.4
```

Bumps `Cargo.toml`/`Cargo.lock`, runs the checks, commits, tags and pushes. The
tag triggers `release.yml`, which builds static binaries for all four platforms,
publishes a GitHub Release and commits the new `release.json` to `main`
(`git pull` afterwards).

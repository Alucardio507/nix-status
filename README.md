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
nixos-status --flake ~/my-config   # use a specific flake.lock (file or directory)
```

### Which channel am I on?
Detection tries, in order: `nix-channel --list` (plus `~/.nix-channels` and
`/root/.nix-channels`), the nixpkgs inputs in `flake.lock` (`/etc/nixos`,
`~/.config/nixos`, `~/.config/home-manager`, or `--flake`; `follows` resolved,
inputs pinned to a bare commit are matched against the channel heads), the
system/user `nix registry` entry, then guesses from the running system's
revision and NixOS version.

### Build errors
For every maintained channel the tool asks Hydra for the latest finished build
of the job that blocks that channel (`nixos/release-XX.YY/tested`,
`nixos/trunk-combined/tested`, ...). If that job failed, the channel is marked
`⚠ BUILD ERRORS` in the table and listed in a footer; if it is one of *your*
channels, a red banner tops the summary together with a link to the failing
build. Failing individual packages that are not part of the `tested` job do not
block a channel and are not reported. Unmaintained channels are skipped, and if
Hydra is unreachable nothing is flagged. `--json` exposes `build_ok`,
`build_problem` and `hydra_build_id`.

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

### Releasing
```sh
just release 0.1.4
```

Bumps `Cargo.toml`/`Cargo.lock`, runs the checks, commits, tags and pushes. The
tag triggers `release.yml`, which builds static binaries for all four platforms,
publishes a GitHub Release and commits the new `release.json` to `main`
(`git pull` afterwards).

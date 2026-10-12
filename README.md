# nixos-status

A small Rust terminal app that shows the public NixOS channel status from
[status.nixos.org](https://status.nixos.org/). It also tries to identify your
local `nix-channel` entries, compares the running system revision with the
channel head, and prints the installed NixOS version when available.

The app uses Rust's `reqwest` crate for HTTPS and `serde_json` for JSON parsing.
It does not shell out to `curl` or `jq`, and the package has no runtime wrapper
dependencies. It still checks `nix-channel` and `nixos-version` opportunistically
for local system information; when those commands are unavailable, the public
channel table can still be shown.

## Run with Nix

Prebuilt binaries are downloaded from GitHub Releases, so nothing is compiled:

```sh
nix run github:YOURNAME/nixos-status
```

Until the first release exists, the flake falls back to building from source.
To force a source build at any time: `nix build .#source`.

## First-time setup

The bundled `Cargo.lock` is only a placeholder. Generate the real lockfiles once:

```sh
nix develop
cargo generate-lockfile
cargo fmt
cargo test
nix flake lock
nix build .#source
```

Commit both `Cargo.lock` and `flake.lock`.

## Develop

```sh
nix develop
cargo run
cargo test
cargo clippy -- -D warnings
```

The program queries the live public Prometheus API, so it needs network access
when it runs.

## Publish to GitHub

1. Create an empty GitHub repository (no README, `.gitignore` or license).
2. Run the **First-time setup** commands above.
3. Commit and push:

   ```sh
   git init -b main
   git add .
   git commit -m "Initial nixos-status flake"
   git remote add origin git@github.com:YOURNAME/nixos-status.git
   git push -u origin main
   ```

4. In the repository on GitHub, open Settings, then Actions, then General, and
   under "Workflow permissions" choose **Read and write permissions**.
   The release workflow needs this to publish releases and update `release.json`.

## Releasing

1. Set `version` in `Cargo.toml` (for example `0.1.0`), commit, and push.
   The CI workflow runs fmt, clippy, tests and a Nix build.
2. Tag and push:

   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```

3. The release workflow builds binaries for x86_64/aarch64 Linux and macOS,
   uploads them to a GitHub Release, computes their hashes, and commits the
   updated `release.json` to `main`.

The tag must match the `Cargo.toml` version (`v0.1.0` for `0.1.0`), otherwise
the workflow stops. Users should follow `main`, which always holds the latest
hashes. To pin, use a commit from `main` made after the release
(`github:YOURNAME/nixos-status/<commit>`), not the tag itself, since the tag
points at the commit before `release.json` was updated.

## Install in a NixOS flake configuration

Add the project as an input in your system flake, keeping your existing
`nixpkgs` input:

```nix
inputs.nixos-status = {
  url = "github:YOURNAME/nixos-status";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Pass `inputs` to your NixOS modules through `specialArgs` in the existing
`nixosSystem` definition:

```nix
specialArgs = { inherit inputs; };
```

Then in `configuration.nix`, install it as a normal system package:

```nix
{ pkgs, inputs, ... }:
{
  environment.systemPackages = [
    inputs.nixos-status.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
```

Rebuild using your host's existing flake configuration, for example:

```sh
sudo nixos-rebuild switch --flake /etc/nixos#YOUR-HOST
```

After the rebuild, run `nixos-status` from your terminal.

## Notes

- Supports `x86_64-linux`, `aarch64-linux`, `x86_64-darwin`, and `aarch64-darwin`.
- ANSI colors are disabled when stdout is not a terminal.
- The status freshness colors are rough indicators: green under two days, yellow
  under five days, and red otherwise.
- For flake-based systems, matching a local revision to a channel head is only a
  heuristic; it does not identify which flake input produced the running system.

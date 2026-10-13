# nixos-status

Small Rust terminal app that shows the public NixOS channel status from
[status.nixos.org](https://status.nixos.org/). It also tries to identify your
local `nix-channel` entries, compares the running system revision with the
channel head, and prints the installed NixOS version when available.

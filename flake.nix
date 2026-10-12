{
  description = "A tiny terminal dashboard for NixOS channel status";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;

      # release.json is written by the release workflow (.github/workflows/release.yml).
      # It holds the version plus the URL and hash of each prebuilt binary.
      release = builtins.fromJSON (builtins.readFile ./release.json);
      cargoVersion = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

      metaFor = pkgs: {
        description = "Show NixOS channel status in the terminal";
        homepage = "https://status.nixos.org/";
        license = pkgs.lib.licenses.mit;
        mainProgram = "nixos-status";
        platforms = systems;
      };

      # Builds from source with cargo. Used for development, CI, and as a
      # fallback before the first release exists.
      sourcePackageFor =
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = "nixos-status";
          version = cargoVersion;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          doCheck = true;
          meta = metaFor pkgs;
        };

      # Downloads the prebuilt binary from GitHub Releases. No compiling.
      binaryPackageFor =
        system: asset:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        pkgs.stdenv.mkDerivation {
          pname = "nixos-status";
          version = release.version;
          src = pkgs.fetchurl { inherit (asset) url hash; };
          dontUnpack = true;
          dontConfigure = true;
          dontBuild = true;
          # Linux binaries are static (musl); do not strip/patch anything,
          # that would also break macOS code signatures.
          dontFixup = true;
          installPhase = ''
            mkdir -p $out/bin
            tar -xzf $src -C $out/bin nixos-status
            chmod +x $out/bin/nixos-status
          '';
          meta = (metaFor pkgs) // {
            sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
          };
        };

      defaultPackageFor =
        system:
        if release.assets ? ${system} then
          binaryPackageFor system release.assets.${system}
        else
          sourcePackageFor system;
    in
    {
      packages = forAllSystems (system: rec {
        default = defaultPackageFor system;
        nixos-status = default;
        # Always compiles from source, ignoring release.json.
        source = sourcePackageFor system;
      });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/nixos-status";
        };
        nixos-status = self.apps.${system}.default;
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
              git
              nixfmt-rfc-style
            ];
          };
        }
      );

      formatter = forAllSystems (system: (import nixpkgs { inherit system; }).nixfmt-rfc-style);
    };
}

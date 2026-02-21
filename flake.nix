{
  description = "cxgen – CLI tool to generate ComicInfo.xml for comic archives";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    flake-utils.url = "github:numtide/flake-utils";

    crane.url = "github:ipetkov/crane";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # Stable Rust toolchain.  Override here to use nightly if needed.
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Filter source: only include files that affect the Rust build.
        # The `comicinfo` and `api` directories are reference material only.
        src = pkgs.lib.cleanSourceWith {
          src = craneLib.cleanCargoSource ./.;
          filter = path: type:
            (craneLib.filterCargoSources path type);
        };

        # Arguments common to all crane derivations.
        commonArgs = {
          inherit src;
          strictDeps = true;

          # Pure-Rust dependencies only (reqwest uses rustls-tls,
          # sevenz-rust and zip are pure Rust) – no pkg-config needed.
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];
        };

        # Build only the dependency crates to populate the Nix store cache.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # The main package derivation.
        cxgen = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });

      in
      {
        # ------------------------------------------------------------------ #
        # Packages
        # ------------------------------------------------------------------ #
        packages = {
          default = cxgen;
          inherit cxgen;
        };

        # ------------------------------------------------------------------ #
        # Apps
        # ------------------------------------------------------------------ #
        apps.default = flake-utils.lib.mkApp {
          drv = cxgen;
          name = "cxgen";
        };

        # ------------------------------------------------------------------ #
        # Checks  (run with `nix flake check`)
        # ------------------------------------------------------------------ #
        checks = {
          # Build the package as a check
          inherit cxgen;

          # Clippy
          cxgen-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });

          # Formatting
          cxgen-fmt = craneLib.cargoFmt { inherit src; };

          # Unit tests
          cxgen-test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
          });
        };

        # ------------------------------------------------------------------ #
        # Dev shell  (`nix develop`)
        # ------------------------------------------------------------------ #
        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          packages = with pkgs; [
            # Handy Cargo extensions
            cargo-edit # cargo add / upgrade
            cargo-watch # cargo watch -x check
            cargo-nextest # faster test runner

            # For inspecting archives during development
            p7zip
            unzip
          ];

          # Surface the Bangumi token from the environment without hardcoding
          # it.  Set this in your shell profile or .envrc:
          #   export BANGUMI_TOKEN=your_token_here
          shellHook = ''
            echo "cxgen dev shell"
            echo "  cargo build     – build the binary"
            echo "  cargo test      – run unit tests"
            echo "  cargo run -- --help"
            echo ""
            echo "Tip: set BANGUMI_TOKEN in your environment to enable"
            echo "     authenticated Bangumi API requests."
          '';
        };
      }
    );
}

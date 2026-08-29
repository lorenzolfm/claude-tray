{
  description = "Which Claude Code sessions are waiting on you, in the system tray";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs @ {
    flake-parts,
    nixpkgs,
    rust-overlay,
    crane,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      # Linux only, like claude-agents and for the same reason once removed: this shells out
      # to it, and it reads /proc. A darwin build would start, publish a tray item and report
      # nothing forever, which is worse than not being offered.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      perSystem = {
        system,
        pkgs,
        ...
      }: let
        craneLib = (crane.mkLib pkgs).overrideToolchain (p:
          p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml);

        # ⚠️ `cleanCargoSource` alone keeps only Rust and Cargo files, so `assets/` never
        # reaches the builder and `include_str!` in src/mark.rs fails with a bare
        # "No such file or directory" — a build error that says nothing about filtering.
        # The mark is source, not a resource, so it has to be let through explicitly.
        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            (craneLib.fileset.commonCargoSources ./.)
            ./assets
          ];
        };

        commonArgs = {
          inherit src;
          strictDeps = true;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        claude-tray = craneLib.buildPackage (commonArgs
          // {
            inherit cargoArtifacts;

            pname = "claude-tray";

            doCheck = false;

            nativeBuildInputs = [pkgs.makeWrapper];

            # The Claude mark needs no font — it is rasterised from `assets/claude-mark.svg`.
            # The badge does, so a font carrying ⊘ and the digits has to be *there*. Pinning it
            # in the wrapper means the tray cannot come up blank because fontconfig on some
            # other machine resolves DejaVu Sans elsewhere.
            #
            # ⚠️ This does NOT reach the menu, and the wording here used to claim it did. The
            # menu is drawn by the tray host with the *system* font stack, so its 🙋 ☕ 🐚 🛸 and
            # its braille spinner need a colour emoji font on the box — Pango tags them
            # `lang=und-zsye` and resolves the `emoji` family, which on this one lands on
            # Noto Color Emoji. Pinning it here would do nothing; there is no variable to set.
            #
            # `claude-agents` is deliberately NOT pinned here. It is the thing this applet
            # reads the world through, and Lorenzo upgrades it on its own cadence; wiring a
            # store path in would mean rebuilding the tray to pick up a producer change.
            # It is looked up on PATH, and a missing one is a state the tray renders as ⊘.
            postInstall = ''
              wrapProgram $out/bin/claude-tray \
                --set-default CLAUDE_TRAY_FONT \
                  ${pkgs.dejavu_fonts}/share/fonts/truetype/DejaVuSans.ttf
            '';

            meta = {
              description = "Which Claude Code sessions are waiting on you, in the system tray";
              homepage = "https://github.com/lorenzolfm/claude-tray";
              license = pkgs.lib.licenses.mit;
              mainProgram = "claude-tray";
              platforms = pkgs.lib.platforms.linux;
            };
          });

        # One derivation for each gate, so CI builds them in parallel and a lint failure
        # does not stop someone who only wants to build the crate.
        gates = {
          inherit claude-tray;

          claude-tray-clippy = craneLib.cargoClippy (commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

          claude-tray-test = craneLib.cargoNextest (commonArgs
            // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";

              # The icon tests need a real font. Without this they are written to skip
              # rather than fail, which would quietly cost the only coverage the renderer
              # has — so hand them one in the sandbox instead.
              CLAUDE_TRAY_FONT = "${pkgs.dejavu_fonts}/share/fonts/truetype/DejaVuSans.ttf";
            });

          claude-tray-fmt = craneLib.cargoFmt {inherit src;};
        };
      in {
        _module.args.pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };

        checks = gates;

        packages =
          gates
          // {
            default = claude-tray;
          };

        apps.default = {
          type = "app";
          program = "${pkgs.lib.getExe claude-tray}";
        };

        devShells.default = craneLib.devShell {
          packages = with pkgs; [
            cargo-nextest
            dejavu_fonts
          ];

          shellHook = ''
            echo "  Rust: $(rustc --version)"
          '';
        };

        formatter = pkgs.alejandra;
      };
    };
}

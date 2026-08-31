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
      # Linux only, like claude-ps, and for the same reason: this program runs claude-ps, and
      # claude-ps reads /proc. A darwin build would start, publish a tray item and report
      # nothing, which is worse than no build at all.
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

        # `cleanCargoSource` alone keeps only Rust and Cargo files, so `assets/` does not reach
        # the builder and `include_str!` in src/mark.rs fails with "No such file or directory",
        # which says nothing about the filter. The mark is source and not a resource, so this
        # code must pass it through.
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

            # The Claude mark needs no font, because src/mark.rs rasterises it from
            # `assets/claude-mark.svg`. The badge does need one, so a font that carries ⊘ and the
            # digits must be present. A pinned path in the wrapper prevents an empty tray if
            # fontconfig on another machine finds DejaVu Sans in a different place.
            #
            # This variable does not reach the menu. The tray host draws the menu with the system
            # fonts, so its 🙋 ☕ 🐚 🛸 and its braille spinner need a colour emoji font on the
            # machine. Pango marks them `lang=und-zsye` and resolves the `emoji` family, which
            # here gives Noto Color Emoji. A pinned path here would have no effect, because there
            # is no variable to set.
            #
            # This wrapper does not pin `claude-ps` on purpose. The applet reads the world
            # through it, and it has its own release cadence. A store path here would need a
            # rebuild of the tray for each change of the producer. It comes from PATH, and the
            # tray shows an absent one as ⊘.
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

        # One derivation for each gate, so that CI builds them in parallel and a lint failure
        # does not stop a build of the crate.
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

              # The icon tests need a real font. Without one they do not run, because they skip
              # instead of a failure, and the renderer then has no test coverage. This variable
              # gives them a font in the sandbox.
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

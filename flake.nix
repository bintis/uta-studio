{
  description = "Uta! Studio AI chart editor and multi-format song exporter";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, crane, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      packages = forAllSystems (pkgs:
        let
          # Native runtimes use the explicitly invoked latest-source builders
          # shipped below. Do not retain a second fixed-wheel XPU implementation.
          # A sandboxed XPU/oneAPI source derivation belongs to release packaging.
          pname = "uta-studio";
          version = (builtins.fromTOML (builtins.readFile ./desktop/Cargo.toml)).package.version;
          # Follow stable through the locked overlay; update that input to
          # upgrade Cargo and rustc without changing the development shell.
          rustPackages = pkgs.extend rust-overlay.overlays.default;
          rustToolchain = rustPackages.rust-bin.stable.latest.minimal;
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              let base = builtins.baseNameOf path; in
              base != "target"
              && base != "test-artifacts"
              && base != ".git"
              && base != "node_modules"
              && base != "dist"
              && base != "result"
              && base != "__pycache__"
              && !(pkgs.lib.hasSuffix ".pyc" base);
          };
          gstPlugins = with pkgs.gst_all_1; [
            gstreamer
            gst-plugins-base
            gst-plugins-good
          ];
          # GStreamer is a multi-output package. The default package path can
          # resolve to its `bin` output, which contains gst-inspect but not the
          # coreelements plugin that provides typefind. Always build the
          # runtime search path from each package's library output.
          gstPluginPath = pkgs.lib.makeSearchPath "lib/gstreamer-1.0"
            (map pkgs.lib.getLib gstPlugins);
          runtimeLibraries = with pkgs; [
            stdenv.cc.cc
            zlib
          ];

          cargoExtraArgs = "--locked -p uta-studio-desktop -p uta-runtime-manager -p uta-fusion-agent-adapter -p uta-analysis-engine -p uta-ggml-worker";

          commonArgs = {
            inherit pname version src cargoExtraArgs;

            nativeBuildInputs = with pkgs; [
              makeWrapper
              pkg-config
            ];

            buildInputs = gstPlugins ++ (with pkgs; [
              libglvnd
              libxkbcommon
              udev
              wayland
              wayland-protocols
              vulkan-loader
            ]);
          };

          # Dependency-only derivation, keyed on Cargo.toml/Cargo.lock via
          # craneLib.cleanCargoSource rather than the full source tree.
          # Editing app code, native-inference scripts, or desktop assets
          # does not change this derivation's input hash, so `nix build`
          # reuses the prebuilt dependency crates instead of recompiling
          # the whole dependency graph from scratch every time.
          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
            src = craneLib.cleanCargoSource src;
          });
        in {
          default = self.packages.${pkgs.stdenv.hostPlatform.system}."uta-studio";

          "uta-studio" = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;

            # The test suite spawns real subprocess trees (ffmpeg, native
            # workers, fake-engine scripts) and is verified separately via
            # `cargo test --workspace`. Running it again inside the build
            # sandbox only adds spurious failures under host contention
            # (fork/exec starvation on a busy machine) without validating
            # anything `cargo test` didn't already cover.
            doCheck = false;

            installPhase = ''
              runHook preInstall
              install -Dm755 target/release/uta-studio $out/bin/.uta-studio-unwrapped
              install -Dm755 target/release/uta-runtime $out/bin/.uta-runtime-unwrapped
              install -Dm755 target/release/uta-fusion-agent-adapter $out/bin/uta-fusion-agent-adapter
              install -Dm755 target/release/uta-fusion-agent-pi $out/bin/uta-fusion-agent-pi
              install -Dm755 target/release/uta-fusion-agent-codex $out/bin/uta-fusion-agent-codex
              install -Dm755 target/release/uta-fusion-agent-claude $out/bin/uta-fusion-agent-claude
              install -Dm644 target/release/uta-fusion-agent-adapter.uta-fusion-adapter.json \
                $out/bin/uta-fusion-agent-adapter.uta-fusion-adapter.json
              install -Dm644 target/release/uta-fusion-agent-pi.uta-fusion-adapter.json \
                $out/bin/uta-fusion-agent-pi.uta-fusion-adapter.json
              install -Dm644 target/release/uta-fusion-agent-codex.uta-fusion-adapter.json \
                $out/bin/uta-fusion-agent-codex.uta-fusion-adapter.json
              install -Dm644 target/release/uta-fusion-agent-claude.uta-fusion-adapter.json \
                $out/bin/uta-fusion-agent-claude.uta-fusion-adapter.json
              install -Dm755 target/release/uta-analyze $out/bin/.uta-analyze-unwrapped
              install -Dm755 target/release/uta-ggml-worker $out/bin/uta-ggml-worker
              install -Dm755 native-inference/ggml-worker/build-ggml-runtime.sh \
                $out/share/uta-studio/native-inference/ggml-worker/build-ggml-runtime.sh
              install -Dm644 native-inference/ggml-worker/runtime-recipe.json \
                $out/share/uta-studio/native-inference/ggml-worker/runtime-recipe.json
              # The optional LibTorch XPU runtime is installed on the machine, like the
              # GGML runtime: ship the latest-source builder, its recipe and
              # native sources. Upstream Python code generation is build-only.
              install -Dm755 native-inference/libtorch-runtime/install-libtorch-xpu-runtime.sh \
                $out/share/uta-studio/native-inference/libtorch-runtime/install-libtorch-xpu-runtime.sh
              install -Dm755 native-inference/libtorch-runtime/compact-native-libraries.sh \
                $out/share/uta-studio/native-inference/libtorch-runtime/compact-native-libraries.sh
              install -Dm644 native-inference/libtorch-runtime/runtime-recipe.json \
                $out/share/uta-studio/native-inference/libtorch-runtime/runtime-recipe.json
              mkdir -p $out/share/uta-studio/native-inference/libtorch-runtime/native
              install -Dm644 native-inference/libtorch-runtime/native/CMakeLists.txt \
                native-inference/libtorch-runtime/native/*.cpp \
                native-inference/libtorch-runtime/native/*.hpp \
                native-inference/libtorch-runtime/native/*.h \
                -t $out/share/uta-studio/native-inference/libtorch-runtime/native
              install -Dm644 icon.png $out/share/uta-studio/icon.png
              install -Dm644 desktop/assets/fonts/NotoSansCJKsc-Regular.otf \
                $out/share/uta-studio/desktop/assets/fonts/NotoSansCJKsc-Regular.otf
              install -Dm644 desktop/assets/icons/ui-icons.svg \
                $out/share/uta-studio/desktop/assets/icons/ui-icons.svg
              install -Dm644 desktop/assets/icons/ui-icons.png \
                $out/share/uta-studio/desktop/assets/icons/ui-icons.png
              install -Dm644 desktop/assets/icons/music-placeholder.png \
                $out/share/uta-studio/desktop/assets/icons/music-placeholder.png
              install -Dm644 desktop/assets/icons/music-placeholder.svg \
                $out/share/uta-studio/desktop/assets/icons/music-placeholder.svg
              install -Dm644 icon.png $out/share/icons/hicolor/512x512/apps/uta-studio.png
              install -Dm644 desktop/uta-studio.desktop $out/share/applications/uta-studio.desktop
              runtimeWrapperArgs=(
                --prefix PATH : /run/current-system/sw/bin
                --set UTA_STUDIO_GGML_RUNTIME_PATH $out/bin/uta-ggml-worker
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}"
                --prefix LD_LIBRARY_PATH : /run/opengl-driver/lib
              )
              makeWrapper $out/bin/.uta-runtime-unwrapped $out/bin/uta-runtime "''${runtimeWrapperArgs[@]}"
              makeWrapper $out/bin/.uta-analyze-unwrapped $out/bin/uta-analyze "''${runtimeWrapperArgs[@]}"
              makeWrapper $out/bin/.uta-studio-unwrapped $out/bin/uta-studio \
                --prefix PATH : /run/current-system/sw/bin \
                --set UTA_STUDIO_ASSET_PATH $out/share/uta-studio \
                --set UTA_STUDIO_ANALYSIS_CLI_PATH $out/bin/uta-analyze \
                --set UTA_STUDIO_RUNTIME_CLI_PATH $out/bin/uta-runtime \
                --set UTA_STUDIO_GGML_RUNTIME_PATH $out/bin/uta-ggml-worker \
                --set WINIT_UNIX_BACKEND wayland \
                --set __EGL_VENDOR_LIBRARY_DIRS /run/opengl-driver/share/glvnd/egl_vendor.d \
                --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "${gstPluginPath}" \
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}" \
                --prefix LD_LIBRARY_PATH : /run/opengl-driver/lib
              runHook postInstall
            '';

            meta = with pkgs.lib; {
              description = "AI-assisted song chart editing with .utz and UltraStar export";
              license = licenses.gpl3Only;
              mainProgram = "uta-studio";
              platforms = platforms.linux;
            };
          });
        });
      devShells = forAllSystems (pkgs:
        let
          gstPlugins = with pkgs.gst_all_1; [
            gstreamer
            gst-plugins-base
            gst-plugins-good
          ];
          gstPluginPath = pkgs.lib.makeSearchPath "lib/gstreamer-1.0"
            (map pkgs.lib.getLib gstPlugins);
          runtimeLibraries = with pkgs; [
            stdenv.cc.cc
            zlib
          ];
        in {
          default = pkgs.mkShell {
            # Development uses the Rust toolchain already installed through
            # rustup. Nix supplies native libraries and runtime tools only,
            # so entering the shell never realizes another pinned rustc.
            packages = gstPlugins ++ (with pkgs; [
              cmake
              ninja
              pkg-config
              libglvnd
              libxkbcommon
              udev
              wayland
              wayland-protocols
              vulkan-headers
              vulkan-loader
            ]);
            shellHook = ''
              if [ -d "$HOME/.cargo/bin" ]; then
                export PATH="$HOME/.cargo/bin:$PATH"
              fi
              # Settings > Models & runtime scans for locally installed AI
              # agent CLIs (claude, codex, gemini, ...) instead of asking the
              # user to browse for one by hand. A binary launched from a
              # desktop icon does not inherit this dev shell's PATH, so it
              # cannot see tools this shell adds (e.g. via ~/.cargo/bin or a
              # nix profile) unless that PATH is captured explicitly here and
              # read back by the scanner as a preferred search path.
              export UTA_STUDIO_AGENT_SEARCH_PATH="$PATH"
              if [ -z "''${UTA_STUDIO_FFMPEG_PATH:-}" ]; then
                if command -v ffmpeg >/dev/null 2>&1; then
                  export UTA_STUDIO_FFMPEG_PATH="$(command -v ffmpeg)"
                fi
              fi
              export UTA_STUDIO_ANALYSIS_CLI_PATH="$PWD/target/debug/uta-analyze"
              export UTA_STUDIO_RUNTIME_CLI_PATH="$PWD/target/debug/uta-runtime"
              export UTA_STUDIO_GGML_RUNTIME_PATH="$PWD/target/debug/uta-ggml-worker"
              export WINIT_UNIX_BACKEND=wayland
              export __EGL_VENDOR_LIBRARY_DIRS=/run/opengl-driver/share/glvnd/egl_vendor.d
              export GST_PLUGIN_SYSTEM_PATH_1_0="${gstPluginPath}:''${GST_PLUGIN_SYSTEM_PATH_1_0:-}"
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}:/run/opengl-driver/lib:''${LD_LIBRARY_PATH:-}"
              # winit dynamically loads Wayland and graphics libraries at runtime.
              # Embed the Nix and driver locations into native host builds so
              # target/debug and target/release executables run directly on NixOS.
              export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-Wl,-rpath,${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}:/run/opengl-driver/lib ''${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS:-}"
              export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-Wl,-rpath,${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}:/run/opengl-driver/lib ''${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS:-}"
            '';
          };
        });
    };
}

{
  description = "Uta! Studio AI chart editor and multi-format song exporter";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, crane }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      packages = forAllSystems (pkgs:
        let
          pname = "uta-studio";
          version = (builtins.fromTOML (builtins.readFile ./desktop/Cargo.toml)).package.version;
          craneLib = crane.mkLib pkgs;
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              let base = builtins.baseNameOf path; in
              base != "target"
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
            gst-plugins-bad
            gst-plugins-ugly
            gst-libav
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

          cargoExtraArgs = "--locked -p uta-studio-desktop -p uta-runtime-manager -p uta-fusion-agent-adapter -p uta-analysis-engine -p uta-ggml-worker -p uta-openvino-worker -p uta-qwen-worker -p uta-game-worker -p uta-jbm-worker -p uta-fcpe-worker -p uta-basic-pitch-worker -p uta-firered-worker -p uta-stars-worker -p uta-rosvot-worker --features uta-game-worker/gpu";

          commonArgs = {
            inherit pname version src cargoExtraArgs;

            nativeBuildInputs = with pkgs; [
              makeWrapper
              pkg-config
            ];

            buildInputs = gstPlugins ++ (with pkgs; [
              ffmpeg-full
              libglvnd
              libxkbcommon
              udev
              wayland
              wayland-protocols
              vulkan-loader
              openssl
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
              install -Dm755 target/release/uta-openvino-worker $out/bin/uta-openvino-worker
              install -Dm755 target/release/uta-ggml-worker $out/bin/uta-ggml-worker
              install -Dm755 target/release/uta-qwen-asr-worker $out/bin/uta-qwen-asr-worker
              install -Dm755 target/release/uta-qwen-align-worker $out/bin/uta-qwen-align-worker
              install -Dm755 target/release/uta-game-worker $out/bin/uta-game-worker
              install -Dm755 target/release/uta-jbm-worker $out/bin/uta-jbm-worker
              install -Dm755 target/release/uta-fcpe-worker $out/bin/uta-fcpe-worker
              install -Dm755 target/release/uta-basic-pitch-worker $out/bin/uta-basic-pitch-worker
              install -Dm755 target/release/uta-firered-worker $out/bin/uta-firered-worker
              install -Dm755 target/release/uta-stars-worker $out/bin/uta-stars-worker
              install -Dm755 target/release/uta-rosvot-worker $out/bin/uta-rosvot-worker
              install -Dm644 native-inference/openvino-worker/THIRD_PARTY_NOTICES.md \
                $out/share/uta-studio/licenses/openvino-worker-THIRD_PARTY_NOTICES.md
              install -Dm644 native-inference/game/THIRD_PARTY_NOTICES.md \
                $out/share/uta-studio/licenses/game-worker-THIRD_PARTY_NOTICES.md
              install -Dm755 native-inference/openvino-worker/build-openvino-runtime.sh \
                $out/share/uta-studio/native-inference/openvino-worker/build-openvino-runtime.sh
              install -Dm644 native-inference/openvino-worker/runtime-recipe.json \
                $out/share/uta-studio/native-inference/openvino-worker/runtime-recipe.json
              install -Dm644 native-inference/openvino-worker/runtime-recipe-ze-experimental.json \
                $out/share/uta-studio/native-inference/openvino-worker/runtime-recipe-ze-experimental.json
              install -Dm644 native-inference/openvino-worker/tools/convert-model.cpp \
                $out/share/uta-studio/native-inference/openvino-worker/tools/convert-model.cpp
              install -Dm644 native-inference/roformer/THIRD_PARTY_NOTICES.md \
                $out/share/uta-studio/licenses/ggml-roformer-THIRD_PARTY_NOTICES.md
              install -Dm755 native-inference/ggml-worker/build-ggml-runtime.sh \
                $out/share/uta-studio/native-inference/ggml-worker/build-ggml-runtime.sh
              install -Dm644 native-inference/ggml-worker/runtime-recipe.json \
                $out/share/uta-studio/native-inference/ggml-worker/runtime-recipe.json
              mkdir -p $out/share/uta-studio/native-inference/roformer
              cp -R native-inference/roformer/. $out/share/uta-studio/native-inference/roformer/
              install -Dm644 native-inference/rmvpe/THIRD_PARTY_NOTICES.md \
                $out/share/uta-studio/licenses/ggml-rmvpe-THIRD_PARTY_NOTICES.md
              mkdir -p $out/share/uta-studio/native-inference/rmvpe
              cp -R native-inference/rmvpe/. $out/share/uta-studio/native-inference/rmvpe/
              install -Dm644 native-inference/qwen-worker/THIRD_PARTY_NOTICES.md \
                $out/share/uta-studio/licenses/qwen-worker-THIRD_PARTY_NOTICES.md
              install -Dm755 native-inference/qwen-worker/build-qwen-engines.sh \
                $out/share/uta-studio/native-inference/qwen-worker/build-qwen-engines.sh
              install -Dm755 native-inference/qwen-worker/install-local-qwen-assets.sh \
                $out/share/uta-studio/native-inference/qwen-worker/install-local-qwen-assets.sh
              install -Dm644 native-inference/qwen-worker/patches/predict-woo-require-gpu.patch \
                $out/share/uta-studio/native-inference/qwen-worker/patches/predict-woo-require-gpu.patch
              install -Dm644 native-inference/qwen-worker/patches/predict-woo-fix-alignment-json-buffer-truncation.patch \
                $out/share/uta-studio/native-inference/qwen-worker/patches/predict-woo-fix-alignment-json-buffer-truncation.patch
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
                --set UTA_STUDIO_FFMPEG_PATH ${pkgs.ffmpeg-full}/bin/ffmpeg
                --set UTA_STUDIO_OPENVINO_RUNTIME_PATH $out/bin/uta-openvino-worker
                --set UTA_STUDIO_GGML_RUNTIME_PATH $out/bin/uta-ggml-worker
                --set UTA_STUDIO_QWEN_ASR_RUNTIME_PATH $out/bin/uta-qwen-asr-worker
                --set UTA_STUDIO_QWEN_ALIGN_RUNTIME_PATH $out/bin/uta-qwen-align-worker
                --set UTA_STUDIO_GAME_RUNTIME_PATH $out/bin/uta-game-worker
                --set UTA_STUDIO_JBM_RUNTIME_PATH $out/bin/uta-jbm-worker
                --set UTA_STUDIO_FCPE_RUNTIME_PATH $out/bin/uta-fcpe-worker
                --set UTA_STUDIO_BASIC_PITCH_RUNTIME_PATH $out/bin/uta-basic-pitch-worker
                --set UTA_STUDIO_FIRERED_RUNTIME_PATH $out/bin/uta-firered-worker
                --set UTA_STUDIO_STARS_RUNTIME_PATH $out/bin/uta-stars-worker
                --set UTA_STUDIO_ROSVOT_RUNTIME_PATH $out/bin/uta-rosvot-worker
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}"
                --prefix LD_LIBRARY_PATH : /run/opengl-driver/lib
              )
              makeWrapper $out/bin/.uta-runtime-unwrapped $out/bin/uta-runtime "''${runtimeWrapperArgs[@]}"
              makeWrapper $out/bin/.uta-analyze-unwrapped $out/bin/uta-analyze "''${runtimeWrapperArgs[@]}"
              makeWrapper $out/bin/.uta-studio-unwrapped $out/bin/uta-studio \
                --set UTA_STUDIO_ASSET_PATH $out/share/uta-studio \
                --set UTA_STUDIO_FFMPEG_PATH ${pkgs.ffmpeg-full}/bin/ffmpeg \
                --set UTA_STUDIO_ANALYSIS_CLI_PATH $out/bin/uta-analyze \
                --set UTA_STUDIO_RUNTIME_CLI_PATH $out/bin/uta-runtime \
                --set UTA_STUDIO_OPENVINO_RUNTIME_PATH $out/bin/uta-openvino-worker \
                --set UTA_STUDIO_GGML_RUNTIME_PATH $out/bin/uta-ggml-worker \
                --set UTA_STUDIO_QWEN_ASR_RUNTIME_PATH $out/bin/uta-qwen-asr-worker \
                --set UTA_STUDIO_QWEN_ALIGN_RUNTIME_PATH $out/bin/uta-qwen-align-worker \
                --set UTA_STUDIO_GAME_RUNTIME_PATH $out/bin/uta-game-worker \
                --set UTA_STUDIO_JBM_RUNTIME_PATH $out/bin/uta-jbm-worker \
                --set UTA_STUDIO_FCPE_RUNTIME_PATH $out/bin/uta-fcpe-worker \
                --set UTA_STUDIO_BASIC_PITCH_RUNTIME_PATH $out/bin/uta-basic-pitch-worker \
                --set UTA_STUDIO_FIRERED_RUNTIME_PATH $out/bin/uta-firered-worker \
                --set UTA_STUDIO_STARS_RUNTIME_PATH $out/bin/uta-stars-worker \
                --set UTA_STUDIO_ROSVOT_RUNTIME_PATH $out/bin/uta-rosvot-worker \
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
            gst-plugins-bad
            gst-plugins-ugly
            gst-libav
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
              ffmpeg-full
              libglvnd
              libxkbcommon
              udev
              wayland
              wayland-protocols
              shaderc
              vulkan-headers
              vulkan-loader
              vulkan-tools
              openssl
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
              export UTA_STUDIO_FFMPEG_PATH="${pkgs.ffmpeg-full}/bin/ffmpeg"
              export UTA_STUDIO_ANALYSIS_CLI_PATH="$PWD/target/debug/uta-analyze"
              export UTA_STUDIO_RUNTIME_CLI_PATH="$PWD/target/debug/uta-runtime"
              export UTA_STUDIO_OPENVINO_RUNTIME_PATH="$PWD/target/debug/uta-openvino-worker"
              export UTA_STUDIO_GGML_RUNTIME_PATH="$PWD/target/debug/uta-ggml-worker"
              export UTA_STUDIO_QWEN_ASR_RUNTIME_PATH="$PWD/target/debug/uta-qwen-asr-worker"
              export UTA_STUDIO_QWEN_ALIGN_RUNTIME_PATH="$PWD/target/debug/uta-qwen-align-worker"
              export UTA_STUDIO_GAME_RUNTIME_PATH="$PWD/target/debug/uta-game-worker"
              export UTA_STUDIO_JBM_RUNTIME_PATH="$PWD/target/debug/uta-jbm-worker"
              export UTA_STUDIO_FCPE_RUNTIME_PATH="$PWD/target/debug/uta-fcpe-worker"
              export UTA_STUDIO_BASIC_PITCH_RUNTIME_PATH="$PWD/target/debug/uta-basic-pitch-worker"
              export UTA_STUDIO_FIRERED_RUNTIME_PATH="$PWD/target/debug/uta-firered-worker"
              export UTA_STUDIO_STARS_RUNTIME_PATH="$PWD/target/debug/uta-stars-worker"
              export UTA_STUDIO_ROSVOT_RUNTIME_PATH="$PWD/target/debug/uta-rosvot-worker"
              export WINIT_UNIX_BACKEND=wayland
              export __EGL_VENDOR_LIBRARY_DIRS=/run/opengl-driver/share/glvnd/egl_vendor.d
              export GST_PLUGIN_SYSTEM_PATH_1_0="${gstPluginPath}:''${GST_PLUGIN_SYSTEM_PATH_1_0:-}"
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}:/run/opengl-driver/lib:''${LD_LIBRARY_PATH:-}"
            '';
          };
        });
    };
}

{
  description = "Uta! Studio lightweight offline development shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
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
            # Rust stays outside Nix so repeated shell entry never realizes a
            # second toolchain. This flake contains only shell metadata and is
            # intentionally isolated from the repository working tree.
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
              export UTA_STUDIO_FFMPEG_PATH="${pkgs.ffmpeg-full}/bin/ffmpeg"
              # Machine-protocol executables are discovered beside the Studio
              # binary. Do not pin them to target/debug here: doing so makes a
              # release Studio launched from this shell inherit a debug analyzer
              # and miss otherwise-present release workers. Individual tests may
              # still set an explicit override on their command line.
              unset UTA_STUDIO_ANALYSIS_CLI_PATH
              unset UTA_STUDIO_RUNTIME_CLI_PATH
              unset UTA_STUDIO_OPENVINO_RUNTIME_PATH
              unset UTA_STUDIO_GGML_RUNTIME_PATH
              unset UTA_STUDIO_QWEN_ASR_RUNTIME_PATH
              unset UTA_STUDIO_QWEN_ALIGN_RUNTIME_PATH
              export WINIT_UNIX_BACKEND=wayland
              export __EGL_VENDOR_LIBRARY_DIRS=/run/opengl-driver/share/glvnd/egl_vendor.d
              export GST_PLUGIN_SYSTEM_PATH_1_0="${gstPluginPath}:''${GST_PLUGIN_SYSTEM_PATH_1_0:-}"
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}:/run/opengl-driver/lib:''${LD_LIBRARY_PATH:-}"
            '';
          };
        });
    };
}

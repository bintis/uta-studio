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
          commonPackages = gstPlugins ++ (with pkgs; [
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
          commonShellHook = ''
            if [ -d "$HOME/.cargo/bin" ]; then
              export PATH="$HOME/.cargo/bin:$PATH"
            fi
            if [ -z "''${UTA_STUDIO_FFMPEG_PATH:-}" ]; then
              if command -v ffmpeg >/dev/null 2>&1; then
                export UTA_STUDIO_FFMPEG_PATH="$(command -v ffmpeg)"
              fi
            fi
            # Machine-protocol executables are discovered beside the Studio
            # binary. Do not pin them to target/debug here: doing so makes a
            # release Studio launched from this shell inherit a debug analyzer
            # and miss otherwise-present release workers. Individual tests may
            # still set an explicit override on their command line.
            unset UTA_STUDIO_ANALYSIS_CLI_PATH
            unset UTA_STUDIO_RUNTIME_CLI_PATH
            unset UTA_STUDIO_GGML_RUNTIME_PATH
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
          rocmRelease = "10.0.0";
          pytorchRelease = "2.13.0";
          rocmArchitecture = "gfx1103";
          rocmWheelIndex = "https://stable.repo.amd.com/rocm/whl-next/";
          python = pkgs.python3;
          virtualenv = pkgs.python3Packages.virtualenv;
          rocmBootstrap = pkgs.writeShellApplication {
            name = "uta-studio-rocm-bootstrap";
            runtimeInputs = [ python virtualenv pkgs.coreutils ];
            text = ''
              if [[ $# -gt 1 ]]; then
                printf 'usage: uta-studio-rocm-bootstrap [new-environment-directory]\n' >&2
                exit 2
              fi
              target="''${1:-''${UTA_STUDIO_ROCM_ENVIRONMENT:-$PWD/test-artifacts/libtorch-rocm/runtime}}"
              if [[ -e "$target" ]]; then
                printf 'ROCm environment already exists; refusing to replace it: %s\n' "$target" >&2
                exit 1
              fi
              parent="$(dirname "$target")"
              cache="$parent/pip-cache"
              mkdir -p "$parent" "$cache"
              export PIP_CACHE_DIR="$cache"
              export PIP_DISABLE_PIP_VERSION_CHECK=1
              virtualenv --python "${python}/bin/python" "$target"
              "$target/bin/python" -m pip install --index-url "${rocmWheelIndex}" \
                "rocm[libraries,devel,device-${rocmArchitecture}]==${rocmRelease}" \
                "torch[device-${rocmArchitecture}]==${pytorchRelease}+rocm${rocmRelease}"
              "$target/bin/rocm-sdk" init
              "$target/bin/python" -m pip freeze > "$target/uta-studio-packages.txt"
              "$target/bin/rocm-sdk" version > "$target/uta-studio-rocm-version.txt"
              printf 'rocm=%s\npytorch=%s\narchitecture=%s\n' \
                "${rocmRelease}" "${pytorchRelease}" "${rocmArchitecture}" \
                > "$target/.uta-studio-ready"
              printf 'ROCm development environment ready: %s\n' "$target"
            '';
          };
          rocmShellHook = commonShellHook + ''
            export UTA_STUDIO_ROCM_RELEASE="${rocmRelease}"
            export UTA_STUDIO_PYTORCH_RELEASE="${pytorchRelease}"
            export UTA_STUDIO_ROCM_ARCHITECTURE="${rocmArchitecture}"
            export UTA_STUDIO_ROCM_WHEEL_INDEX="${rocmWheelIndex}"
            # On gfx1103, real RoFormer work uses code-owned synchronized,
            # paced HIP submissions without changing its GPU-only arithmetic.
            export UTA_STUDIO_LIBTORCH_STAGE_SYNC=1
            export UTA_STUDIO_LIBTORCH_STAGE_PACING=1
            export UTA_STUDIO_ROCM_ENVIRONMENT="''${UTA_STUDIO_ROCM_ENVIRONMENT:-$PWD/test-artifacts/libtorch-rocm/runtime}"
            if [ -f "$UTA_STUDIO_ROCM_ENVIRONMENT/.uta-studio-ready" ]; then
              export PATH="$UTA_STUDIO_ROCM_ENVIRONMENT/bin:$PATH"
              export UTA_STUDIO_ROCM_ROOT="$("$UTA_STUDIO_ROCM_ENVIRONMENT/bin/rocm-sdk" path --root)"
              export UTA_STUDIO_ROCM_TORCH_ROOT="$("$UTA_STUDIO_ROCM_ENVIRONMENT/bin/python" -c 'import pathlib, sysconfig; print(pathlib.Path(sysconfig.get_paths()["purelib"]) / "torch")')"
              rocm_site_packages="$(dirname "$UTA_STUDIO_ROCM_ROOT")"
              export UTA_STUDIO_ROCM_LIBRARY_PATH="$UTA_STUDIO_ROCM_TORCH_ROOT/lib:$UTA_STUDIO_ROCM_ROOT/lib:$UTA_STUDIO_ROCM_ROOT/lib/host-math/lib:$UTA_STUDIO_ROCM_ROOT/lib/rocm_sysdeps/lib:$UTA_STUDIO_ROCM_ROOT/lib/llvm/lib:$rocm_site_packages/_rocm_sdk_core/lib:$rocm_site_packages/_rocm_sdk_libraries/lib"
              export LD_LIBRARY_PATH="$UTA_STUDIO_ROCM_LIBRARY_PATH:$LD_LIBRARY_PATH"
              export CMAKE_PREFIX_PATH="$("$UTA_STUDIO_ROCM_ENVIRONMENT/bin/rocm-sdk" path --cmake):''${CMAKE_PREFIX_PATH:-}"
            fi
          '';
        in {
          # Rust stays outside Nix so repeated shell entry never realizes a
          # second toolchain. This flake contains only shell metadata and is
          # intentionally isolated from the repository working tree.
          default = pkgs.mkShell {
            packages = commonPackages;
            shellHook = commonShellHook;
          };

          # ROCm 10 is newer than the pinned nixpkgs ROCm package. Nix owns the
          # host tools; AMD's stable multi-architecture wheels own one isolated
          # gfx1103 SDK/LibTorch environment created by the explicit bootstrap.
          rocm = pkgs.mkShell {
            packages = commonPackages ++ [ python virtualenv rocmBootstrap pkgs.xz ];
            shellHook = rocmShellHook;
          };
        });
    };
}

{
  description = "Uta Studio AI chart editor and multi-format song exporter";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      packages = forAllSystems (pkgs:
        let
          pname = "uta-studio";
          version = (builtins.fromTOML (builtins.readFile ./desktop/Cargo.toml)).package.version;
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
        in {
          default = self.packages.${pkgs.stdenv.hostPlatform.system}."uta-studio";

          "uta-studio" = pkgs.rustPlatform.buildRustPackage {
            inherit pname version src;

            cargoLock = { lockFile = ./Cargo.lock; };

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
              python311
              openssl
              uv
            ]);

            buildPhase = ''
              runHook preBuild
              cargo build --release --locked -p uta-studio-desktop
              runHook postBuild
            '';

            installPhase = ''
              runHook preInstall
              install -Dm755 target/release/uta-studio $out/bin/.uta-studio-unwrapped
              install -Dm644 icon.png $out/share/uta-studio/icon.png
              install -Dm644 desktop/assets/fonts/NotoSansCJKsc-Regular.otf \
                $out/share/uta-studio/desktop/assets/fonts/NotoSansCJKsc-Regular.otf
              install -Dm644 desktop/assets/icons/ui-icons.svg \
                $out/share/uta-studio/desktop/assets/icons/ui-icons.svg
              install -Dm644 desktop/assets/icons/ui-icons.png \
                $out/share/uta-studio/desktop/assets/icons/ui-icons.png
              install -Dm644 icon.png $out/share/icons/hicolor/512x512/apps/uta-studio.png
              install -Dm644 desktop/uta-studio.desktop $out/share/applications/uta-studio.desktop
              makeWrapper $out/bin/.uta-studio-unwrapped $out/bin/uta-studio \
                --set UTA_STUDIO_ASSET_PATH $out/share/uta-studio \
                --set UTA_STUDIO_FFMPEG_PATH ${pkgs.ffmpeg-full}/bin/ffmpeg \
                --set UTA_STUDIO_UV_PATH ${pkgs.uv}/bin/uv \
                --set UTA_STUDIO_PYTHON_PATH ${pkgs.python311}/bin/python3.11 \
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
          };
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
              python311
              openssl
              uv
            ]);
            shellHook = ''
              if [ -d "$HOME/.cargo/bin" ]; then
                export PATH="$HOME/.cargo/bin:$PATH"
              fi
              export UTA_STUDIO_FFMPEG_PATH="${pkgs.ffmpeg-full}/bin/ffmpeg"
              export UTA_STUDIO_UV_PATH="${pkgs.uv}/bin/uv"
              export UTA_STUDIO_PYTHON_PATH="${pkgs.python311}/bin/python3.11"
              export WINIT_UNIX_BACKEND=wayland
              export __EGL_VENDOR_LIBRARY_DIRS=/run/opengl-driver/share/glvnd/egl_vendor.d
              export GST_PLUGIN_SYSTEM_PATH_1_0="${gstPluginPath}:''${GST_PLUGIN_SYSTEM_PATH_1_0:-}"
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}:/run/opengl-driver/lib:''${LD_LIBRARY_PATH:-}"
            '';
          };
        });
    };
}

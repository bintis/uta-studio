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
          version = (builtins.fromTOML (builtins.readFile ./client/src-tauri/Cargo.toml)).package.version;
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
          pnpmDeps = pkgs.fetchPnpmDeps {
            pname = "${pname}-frontend";
            inherit version;
            src = "${src}/client";
            fetcherVersion = 4;
            hash = "sha256-ST0T1IALxjENboRbLIIITCmcR0Cdwh/sPUpfPbTLCpk=";
          };
          gstPlugins = with pkgs.gst_all_1; [
            gst-plugins-base
            gst-plugins-good
            gst-plugins-bad
            gst-plugins-ugly
            gst-libav
          ];
          runtimeLibraries = with pkgs; [
            stdenv.cc.cc
            zlib
          ];
        in {
          default = self.packages.${pkgs.stdenv.hostPlatform.system}."uta-studio";

          "uta-studio" = pkgs.rustPlatform.buildRustPackage {
            inherit pname version src;

            cargoLock = { lockFile = ./Cargo.lock; };
            pnpmDeps = pnpmDeps;
            pnpmRoot = "client";

            nativeBuildInputs = with pkgs; [
              nodejs
              pnpm
              pnpmConfigHook
              pkg-config
              wrapGAppsHook3
            ];

            buildInputs = gstPlugins ++ (with pkgs; [
              alsa-lib
              ffmpeg-full
              gsettings-desktop-schemas
              gtk3
              libayatana-appindicator
              python311
              librsvg
              openssl
              uv
              webkitgtk_4_1
            ]);

            preFixup = ''
              gappsWrapperArgs+=(--set UTA_STUDIO_FFMPEG_PATH ${pkgs.ffmpeg-full}/bin/ffmpeg)
              gappsWrapperArgs+=(--set UTA_STUDIO_UV_PATH ${pkgs.uv}/bin/uv)
              gappsWrapperArgs+=(--set UTA_STUDIO_PYTHON_PATH ${pkgs.python311}/bin/python3.11)
              # WebKitGTK loads codecs and audio sinks through GStreamer at
              # runtime. Merely adding the plugins to buildInputs does not put
              # their store paths in GStreamer's search path (or even retain
              # them in the final runtime closure).
              gappsWrapperArgs+=(--prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "${pkgs.lib.makeSearchPath "lib/gstreamer-1.0" gstPlugins}")
              # PyPI wheels such as NumPy need basic native runtime libraries.
              # GPU runtimes are supplied by the host so this package remains
              # backend-neutral across Intel, NVIDIA, AMD, and CPU-only hosts.
              gappsWrapperArgs+=(--prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath runtimeLibraries}")
              gappsWrapperArgs+=(--prefix LD_LIBRARY_PATH : /run/opengl-driver/lib)
            '';

            preBuild = ''
              cd client
              pnpm install --offline --frozen-lockfile
              pnpm build
              cd ..
            '';

            buildPhase = ''
              runHook preBuild
              cargo build --release --locked -p uta-studio --features custom-protocol
              runHook postBuild
            '';

            installPhase = ''
              install -Dm755 target/release/uta-studio $out/bin/uta-studio
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
            gst-plugins-base
            gst-plugins-good
            gst-plugins-bad
            gst-plugins-ugly
            gst-libav
          ];
          runtimeLibraries = with pkgs; [
            stdenv.cc.cc
            zlib
          ];
        in {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}."uta-studio" ];
            packages = [ pkgs.rustfmt ];
            shellHook = ''
              # Native GTK open/save/folder dialogs abort the whole process when
              # this schema is absent. The release wrapper gets the same paths
              # from wrapGAppsHook3; keep `cargo run` and `tauri dev` identical.
              export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:''${XDG_DATA_DIRS:-}"
              export UTA_STUDIO_FFMPEG_PATH="${pkgs.ffmpeg-full}/bin/ffmpeg"
              export UTA_STUDIO_UV_PATH="${pkgs.uv}/bin/uv"
              export UTA_STUDIO_PYTHON_PATH="${pkgs.python311}/bin/python3.11"
              export GST_PLUGIN_SYSTEM_PATH_1_0="${pkgs.lib.makeSearchPath "lib/gstreamer-1.0" gstPlugins}:''${GST_PLUGIN_SYSTEM_PATH_1_0:-}"
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibraries}:/run/opengl-driver/lib:''${LD_LIBRARY_PATH:-}"
            '';
          };
        });
    };
}

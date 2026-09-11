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
          # The native LibTorch XPU runtime as a Nix package: the official
          # release archives are fixed-output fetches, unpacked for their
          # shared libraries and C++ headers only (no Python), and the
          # app-owned native library is built against them. The result is the
          # same runtime directory layout install-libtorch-xpu-runtime.sh
          # produces; point UTA_STUDIO_LIBTORCH_RUNTIME_DIR at
          # $out/share/uta-studio/runtime/libtorch-xpu or link it into the
          # managed runtime store. It is not a dependency of the application
          # package, so the default closure stays small.
          libtorchXpuRuntime = pkgs.stdenv.mkDerivation rec {
            pname = "uta-libtorch-xpu-runtime";
            version = "2.13.0-xpu";
            torchWheel = pkgs.fetchurl {
              name = "torch-2.13.0+xpu-cp311-cp311-manylinux_2_28_x86_64.whl";
              url = "https://download.pytorch.org/whl/xpu/torch-2.13.0%2Bxpu-cp311-cp311-manylinux_2_28_x86_64.whl";
              sha256 = "4a1955ea8196ea5aac1bd009aec34c7f1cc9d1558bf71e1dc5a6f2db626a03dd";
            };
            dependencyWheels = map (wheel: pkgs.fetchurl { inherit (wheel) name url sha256; }) [
              { name = "intel_pti-0.17.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/0b/02/798ea3cb0189b66cef0fc95d9b36f43df740714997f5e0976e074274a270/intel_pti-0.17.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "1a3327b8683af72e60e1ea8f754160fb12ffb8a15e57941f7e3672bcc540e2fc"; }
              { name = "intel_cmplr_lib_ur-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/15/6d/b981353d0ba8dc6d54510aba97f67ddce3872a693f41841bb77a120f512c/intel_cmplr_lib_ur-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "5fb75293e7f1f8377cda3aac44f4e7c0c1dd38e281fecefa76ca1459382763a4"; }
              { name = "intel_sycl_rt-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/3a/7b/b70751dd0105741fdd23d064243291badf91cad9fd7375d1640893672524/intel_sycl_rt-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "5ae2bf7ec928fe127ad693dd84d8fed27209be0790e36eabffb93a371ba174aa"; }
              { name = "onemkl_sycl_dft-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/5f/c1/06fc9c690ed9122e9e616d3088f06bf33367ed1a45a9485a30a4682b5004/onemkl_sycl_dft-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "e6eabc72f8311d8faad9c841cd6e966680134dd94308d58f33e9cc06900d733d"; }
              { name = "tcmlib-1.5.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/60/24/aa409bb20703acc70cf4d3bc620a55c789639c2995b2667fb44ae7236ec9/tcmlib-1.5.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "9d7c01cff35aae9bf5390b620680ebdf10a7d211c22d6488a27a029502e7d0aa"; }
              { name = "intel_openmp-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/6b/52/ad8da758c96299c27ac1f0345979f9202a517c4f18ef6a1e9b7a781d6948/intel_openmp-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "c4605c63840d6dc0188610f9d0dcc5ae6c73c988f98c36c4bd807bcbd0de73bb"; }
              { name = "onemkl_sycl_blas-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/71/41/d22e73b1258611e174ef8dfdb21dd34645d97f06b7483df89f824a6b661c/onemkl_sycl_blas-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "f3c369f69f3a17ac4b01c6dc31159505c8fac46a2e2f681221462578ea036109"; }
              { name = "onemkl_sycl_rng-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/76/8b/3dece5b2f41b9e30b0ec77f336099493eefc43bd0c811ac9f6550a86c369/onemkl_sycl_rng-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "b9fbd260ac3dcf52d50ab5abb89760d871ba1fab7f613caf68e265d7e3221841"; }
              { name = "onemkl_sycl_lapack-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/79/41/5f471e4c2333dfa4641aa94bc432a369268b6223c333b5a38d1f97505ce6/onemkl_sycl_lapack-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "f128bc8142082a9de735a6d0b8e2de3d8ca145db57616e2dd8d2a26f5df1714b"; }
              { name = "mkl-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/91/97/06ad1072db8a3c4d1e6237badf660c4c754d2b513264996121c2b666d65d/mkl-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "4a9525ddd671b422fedfc690dd9270beb65c8cbd47044b52d2a4e3f6162c9de6"; }
              { name = "oneccl-2022.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/94/cb/c8918376dfba8392db992ac9cfac7f201b4ebb09178de3b7f33d98e3d745/oneccl-2022.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "31a122f0b46da841bf73ab6ccac1bd11e7d2b9d9ec7b84c8cf645d48c2095e8d"; }
              { name = "onemkl_sycl_sparse-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/9a/aa/bf9bf4b05ee52ecbda5d8260ac3e579b61a360a81750d2c5aba57a8996e5/onemkl_sycl_sparse-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "ee1e38be9c872d75bb61181b7791e67dd29b010dc20a2cd59716e1e0b4d45328"; }
              { name = "impi_rt-2021.18.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/9c/92/ccef0ec3b9bd9f3cf586abaf863bcc6b7b7c0dd4868fe5b41e6155f03608/impi_rt-2021.18.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "7bd0328b89872f9395523c3732eb97b403b2e04a4210c85016eac87301398fbc"; }
              { name = "tbb-2023.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/aa/d2/9a994ce9b18182b04783282eba77e236d23919acf42a886d72fe14fc78a4/tbb-2023.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "482f57656386ea14b96e8da36b3fcc4cd880834ef0f328ba09e8e2e3c639285e"; }
              { name = "umf-1.1.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/c4/72/2e0182f4e6a727a15d0a8a99a82182a4f5bdec1a4f5767acfd2abdc72070/umf-1.1.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "567152c5ee6b8e16cc56b29a8a9a6b918de1febf89877f71ea2e8247ef39fc32"; }
              { name = "intel_cmplr_lib_rt-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; url = "https://files.pythonhosted.org/packages/e1/61/50d3fb0deb97f5d57803c6bd9dcd1b6ac638e761ae16bea9d71c5187c3a3/intel_cmplr_lib_rt-2026.0.0-py2.py3-none-manylinux_2_28_x86_64.whl"; sha256 = "a8ff1f8ec28a50dddd02e47c6d47eb6b758842103571956f419a53ebb6513d2d"; }
            ];
            src = pkgs.lib.cleanSourceWith {
              src = ./native-inference/libtorch-runtime/native;
              filter = path: type: true;
            };
            nativeBuildInputs = with pkgs; [ cmake ninja unzip ];
            dontUnpack = true;
            # The CMake hook must not configure the source before the archives
            # are unpacked; the build phase configures explicitly.
            dontConfigure = true;
            # The archives' libraries keep their own load layout; stripping or
            # rewriting them is neither needed nor safe.
            dontStrip = true;
            dontPatchELF = true;
            dontFixup = true;
            buildPhase = ''
              runHook preBuild
              runtime=$out/share/uta-studio/runtime/libtorch-xpu
              mkdir -p "$runtime/torch" "$runtime/deps/lib" "$runtime/lib" staging/torch staging/deps
              unzip -q "$torchWheel" 'torch/include/*' 'torch/lib/*' 'torch/share/cmake/*' -d staging/torch
              mv staging/torch/torch/include staging/torch/torch/lib staging/torch/torch/share "$runtime/torch/"
              rm -f "$runtime/torch/lib/libtorch_python.so"
              for wheel in $dependencyWheels; do
                unzip -q -o "$wheel" -d staging/deps
              done
              while IFS= read -r -d "" directory; do
                cp -a "$directory"/. "$runtime/deps/lib/"
              done < <(find staging/deps -type d -name lib -print0)
              find "$runtime/deps/lib" \( -name '*.py' -o -name '*.pyc' -o -name '*.cpython-*.so' \) -delete
              cp -a ${pkgs.level-zero}/lib/libze_loader.so* "$runtime/deps/lib/"
              cp -a ${pkgs.ocl-icd}/lib/libOpenCL.so* "$runtime/deps/lib/"
              cp -L ${pkgs.zlib}/lib/libz.so.1 "$runtime/deps/lib/libz.so.1"
              cmake -S "$src" -B build -G Ninja \
                -DCMAKE_BUILD_TYPE=Release \
                -DTORCH_ROOT="$runtime/torch" \
                -DXPU_DEPENDENCY_LIB="$runtime/deps/lib" \
                -DUTA_LIBTORCH_BACKEND=xpu \
                -DTORCH_CXX_ABI=1
              cmake --build build --target uta_libtorch
              cp build/libuta_libtorch.so "$runtime/lib/libuta_libtorch.so"
              {
                printf '{\n  "backend": "libtorch_xpu",\n  "torch_release": "2.13.0+xpu",\n'
                printf '  "native_library": "lib/libuta_libtorch.so",\n  "packaged_by": "nix",\n'
                printf '  "environment": {\n'
                printf '    "ONEAPI_DEVICE_SELECTOR": "level_zero:gpu",\n'
                printf '    "SYCL_CACHE_PERSISTENT": "1",\n'
                printf '    "ONEDNN_DEFAULT_FPMATH_MODE": "strict",\n'
                printf '    "DNNL_DEFAULT_FPMATH_MODE": "strict",\n'
                printf '    "LD_LIBRARY_PATH": "%s/deps/lib:%s/torch/lib:/run/opengl-driver/lib",\n' "$runtime" "$runtime"
                printf '    "OCL_ICD_VENDORS": "/run/opengl-driver/etc/OpenCL/vendors"\n  },\n'
                printf '  "libraries": {\n'
                first=1
                for library in "$runtime"/lib/*.so "$runtime"/torch/lib/*.so*; do
                  [ -f "$library" ] || continue
                  if [ $first = 1 ]; then first=0; else printf ',\n'; fi
                  printf '    "%s": "%s"' "''${library#"$runtime"/}" "$(sha256sum "$library" | cut -d ' ' -f 1)"
                done
                printf '\n  }\n}\n'
              } > "$runtime/runtime-manifest.json"
              runHook postBuild
            '';
            installPhase = "true";
            meta = with pkgs.lib; {
              description = "Native LibTorch XPU runtime for Uta! Studio (Intel Arc, no Python)";
              license = licenses.bsd3;
              platforms = [ "x86_64-linux" ];
            };
          };
          pname = "uta-studio";
          version = (builtins.fromTOML (builtins.readFile ./desktop/Cargo.toml)).package.version;
          craneLib = crane.mkLib pkgs;
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

          cargoExtraArgs = "--locked -p uta-studio-desktop -p uta-runtime-manager -p uta-fusion-agent-adapter -p uta-analysis-engine -p uta-ggml-worker";

          commonArgs = {
            inherit pname version src cargoExtraArgs;

            nativeBuildInputs = with pkgs; [
              makeWrapper
              pkg-config
            ];

            buildInputs = gstPlugins ++ (with pkgs; [
              ffmpeg
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
          "uta-libtorch-xpu-runtime" = libtorchXpuRuntime;

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
              # GGML runtime: ship the Python-free installer, its recipe and the
              # app-owned native sources it builds against the official archives.
              install -Dm755 native-inference/libtorch-runtime/install-libtorch-xpu-runtime.sh \
                $out/share/uta-studio/native-inference/libtorch-runtime/install-libtorch-xpu-runtime.sh
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
                --set UTA_STUDIO_FFMPEG_PATH ${pkgs.ffmpeg}/bin/ffmpeg
                --set UTA_STUDIO_GGML_RUNTIME_PATH $out/bin/uta-ggml-worker
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}"
                --prefix LD_LIBRARY_PATH : /run/opengl-driver/lib
              )
              makeWrapper $out/bin/.uta-runtime-unwrapped $out/bin/uta-runtime "''${runtimeWrapperArgs[@]}"
              makeWrapper $out/bin/.uta-analyze-unwrapped $out/bin/uta-analyze "''${runtimeWrapperArgs[@]}"
              makeWrapper $out/bin/.uta-studio-unwrapped $out/bin/uta-studio \
                --set UTA_STUDIO_ASSET_PATH $out/share/uta-studio \
                --set UTA_STUDIO_FFMPEG_PATH ${pkgs.ffmpeg}/bin/ffmpeg \
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
                elif [ -x "${pkgs.ffmpeg}/bin/ffmpeg" ]; then
                  export UTA_STUDIO_FFMPEG_PATH="${pkgs.ffmpeg}/bin/ffmpeg"
                fi
              fi
              export UTA_STUDIO_ANALYSIS_CLI_PATH="$PWD/target/debug/uta-analyze"
              export UTA_STUDIO_RUNTIME_CLI_PATH="$PWD/target/debug/uta-runtime"
              export UTA_STUDIO_GGML_RUNTIME_PATH="$PWD/target/debug/uta-ggml-worker"
              export WINIT_UNIX_BACKEND=wayland
              export __EGL_VENDOR_LIBRARY_DIRS=/run/opengl-driver/share/glvnd/egl_vendor.d
              export GST_PLUGIN_SYSTEM_PATH_1_0="${gstPluginPath}:''${GST_PLUGIN_SYSTEM_PATH_1_0:-}"
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath (runtimeLibraries ++ [ pkgs.libglvnd pkgs.libxkbcommon pkgs.udev pkgs.vulkan-loader pkgs.wayland ])}:/run/opengl-driver/lib:''${LD_LIBRARY_PATH:-}"
            '';
          };
        });
    };
}

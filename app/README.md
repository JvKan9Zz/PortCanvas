# PortCanvas native application

The v0.6.4 application uses Slint/winit/FemtoVG, native VT and RFB clients, platform media playback and shared Rust services. It has no frontend bundle, npm install, Tauri or WebView build step. The release package targets Windows x64. The native macOS test build targets macOS 13.0+ on Apple Silicon and Intel. See [Mac build and use](../docs/macos.md).

## Build

Use Rust 1.90+, Git Bash, Visual Studio C++ Build Tools and the Windows SDK. Initialize the MSVC environment before setting project-specific paths. From `app`:

```bash
bash tools/build-native.sh
```

The release binary is `native-probe/target/x86_64-pc-windows-msvc/release/PortCanvasNative.exe`, or the corresponding path beneath `CARGO_TARGET_DIR`. The release entry applies the static Windows CRT consistently to target libraries and the application. No separate VC runtime installer or Node/Python runtime is required by the installed application.

The current-user installer is built separately:

```bash
python tools/native_installer.py --binary native-probe/target/x86_64-pc-windows-msvc/release/PortCanvasNative.exe --output target/PortCanvas_0.6.4_x64-setup.exe
```

NSIS must be available through `--makensis`, PATH, or its existing local cache. The installer uses PortCanvas installation, shortcut and registry identities. Signing uses `tools/native_sign.py` with the existing protected updater key and password file. Never commit those inputs. `latest.json` carries the version-bound signature for the stable update channel. Building does not publish or install a release.

## Check

Create an isolated existing directory and set an absolute `WCS_TEST_ROOT`. In Git Bash on Windows:

```bash
mkdir -p native-probe/target/local-tests
export WCS_TEST_ROOT="$(cygpath -w "$PWD/native-probe/target/local-tests")"
cargo test --manifest-path core/Cargo.toml --locked --features downloads
cargo test --manifest-path native-probe/Cargo.toml --locked --lib --tests
python -B -m unittest discover -s tests -p 'test_*.py'
```

Windows CI checks shared services, protocol ownership, native controls/codecs and the standalone release import table. CI fixtures do not connect a controller, operate hardware, sign or publish. Opt-in real-device checks and their evidence boundaries are described in [the native workbench guide](native-probe/README.md).

## Source

- `core`: SSH, SFTP, tmux/PTY, logs, configuration, environment preparation and native desktop/camera transports.
- `native-probe`: the native workbench, models, system integration, installer and isolated acceptance fixtures. `PortCanvasNative` is the application; `PortCanvasNativeProbe` is an explicitly invoked diagnostic binary.
- `vendor/vnc-rs`: the retained native RFB source and patch record.
- `tools`: current build, installer, notice and signing utilities.

Fresh Windows installs use `data/` under the installation directory, with default logs in `data/logs/`. In-place Windows upgrades keep existing Stable data in use. On macOS, Stable uses `~/Library/Application Support/PortCanvas/Stable`. The Beta channel has separate storage. For isolated checks, set `WCS_DATA_DIR`. Credentials use Windows DPAPI or a macOS Keychain-protected encryption key and stay on the original computer during `.portcanvas` export. Imports use the configuration and media transaction with preview and rollback. Source for old browser clients remains available in Git history.

UI text uses platform native system fonts (`Microsoft YaHei UI` on Windows, `PingFang SC` on macOS) without bundling partial CJK subsets. The pinned 1.15.1 FemtoVG renderer includes the text-origin and baseline alignment patches documented under `vendor/i-slint-renderer-femtovg/PATCHES.md`. This does not install system fonts.

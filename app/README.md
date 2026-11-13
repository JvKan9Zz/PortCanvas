# PortCanvas Windows native application

The v0.6.0-beta.1 development line uses Slint/winit/FemtoVG, native VT and RFB clients, Windows Media Foundation and shared Rust services. It has no frontend bundle, npm install, Tauri or WebView build step. The preview is not yet published; M7 acceptance remains in progress. Windows x64 is the current target. Historical macOS v0.5.95 builds remain in the tagged release.

## Build

Use Rust 1.90+, Git Bash, Visual Studio C++ Build Tools and the Windows SDK. Initialize the MSVC environment before setting project-specific paths. From `app`:

```bash
bash tools/build-native.sh
```

The release binary is `native-probe/target/x86_64-pc-windows-msvc/release/PortCanvasNative.exe`, or the corresponding path beneath `CARGO_TARGET_DIR`. The release entry applies the static Windows CRT consistently to target libraries and the application. No separate VC runtime installer or Node/Python runtime is required by the installed application.

The current-user installer is built separately:

```bash
python tools/native_installer.py --binary native-probe/target/x86_64-pc-windows-msvc/release/PortCanvasNative.exe --output target/PortCanvas_Beta_0.6.0-beta.1_x64-setup.exe
```

NSIS must be available through `--makensis`, PATH, or its existing local cache. The installer uses independent Beta installation, shortcut and registry identities. Signing uses `tools/native_sign.py` with the existing protected updater key and password file. Never commit those inputs. `beta.json` carries the version-bound signature; Beta does not use the stable `latest.json` channel. Building does not publish or install a release.

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

The default application data directory is `%LOCALAPPDATA%/PortCanvas/Beta`; `WCS_DATA_DIR` overrides it for isolated checks. Credentials use Windows DPAPI. Import explicitly reviews four configuration files and referenced managed media, preserves the source and supports exact rollback. Native desktop/camera/background rendering starts no local HTTP or WebSocket presentation bridge. Old browser clients and their build files remain available in version tags and Git history.

# macOS native test build

PortCanvas v0.6.1 has a native macOS build using shared Rust services, Slint/winit/FemtoVG, AppKit window integration and AVFoundation background playback. It targets macOS 13.0 and newer. One Universal app contains `arm64` and `x86_64` slices. A deployment target and static inspection do not establish runtime testing on every supported OS or processor; the PR's acceptance record identifies the systems actually exercised.

## Get and install the test build

Open the PR's **macOS native** Actions check, open its workflow run, and download **PortCanvas-0.6.1-macOS-universal-test** from Artifacts. GitHub may require sign-in. The artifact expires after 14 days and can be recreated by rerunning the workflow. This is a PR test package, not an official Mac Release.

Extract the Actions ZIP, open `PortCanvas_0.6.1_universal.dmg`, and drag `PortCanvas.app` to Applications. Quit any running PortCanvas before replacing an installed copy. Open the copy in Applications; running from the mounted disk image is unsuitable for later app replacement. Eject the disk image after installation. The `.app.tar.gz` is the signed-update payload format, not the disk image; its update signature is a separate publisher-managed signature.

The app is **ad-hoc signed and not Apple notarized**. macOS may block its first launch after download. After verifying the source, use System Settings → Privacy & Security → Open Anyway for this app and complete macOS's confirmation. Do not disable Gatekeeper globally. No Developer ID certificate or notarization credential is used by CI. `SHA256SUMS` checks transfer integrity; it does not establish publisher identity. The archive's app signature can be inspected with:

```bash
codesign --verify --deep --strict --verbose=2 /Applications/PortCanvas.app
codesign -d --verbose=4 /Applications/PortCanvas.app
```

## Data, first migration and recovery

Stable data lives in `~/Library/Application Support/PortCanvas/Stable`; Beta uses `Beta` separately. `WCS_DATA_DIR` selects an absolute isolated data directory for testing. Quit the application before making a backup of its data directory. Removing the application does not remove your data.

Stable uses bundle identifier `com.portcanvas.desktop` and installs as `PortCanvas.app`. Beta uses `com.portcanvas.desktop.beta` and the DMG installs `PortCanvas Beta.app`, so both applications can coexist. Their data and update channels remain separate; an updater refuses a cross-channel replacement. Beta build versions use `x.y.z-beta.N` with Apple build counter N from 1 through 255. The numeric `CFBundleShortVersionString` and Apple `CFBundleVersion` remain valid, while `PortCanvasVersion` retains the complete signed semantic version. Older Stable bundles without this key remain update-compatible. The updater archive retains its internal `PortCanvas.app` root for both channels; install Beta from its DMG to obtain the independent application filename.

The historical v0.5.95 Mac app uses a different implementation and manual updates. Install the native app manually, then use Devices → Import legacy data to choose the old directory containing `devices.json`. Review the preview before confirming; source files remain available and migration has a restore operation. Keep original managed background files until import and reopening have been checked. Windows DPAPI credentials require password re-entry on Mac. Same-account Mac credentials rely on the existing Keychain identity; macOS may request permission for access. An owner must complete system Keychain prompts.

The native updater selects `darwin-aarch64` or `darwin-x86_64`, verifies the complete downloaded archive against the signed version, and replaces the full installed application before relaunch. A separate helper waits for the old application to exit, exchanges the bundles atomically, and waits for the replacement to render before removing old code. Failed startup stops the new process, restores the old bundle and restarts it. User data is retained. Updates require a writable installed location and an original bundle this account can remove; locked, read-only, linked or differently owned installations require a manual app replacement. The incoming bundle must match the installed signing team (including ad-hoc status). A channel with only Windows entries means no applicable Mac update. The public Windows `latest.json` is unchanged by Mac test builds. Publisher update keys are not generated or replaced by this build. If installation or replacement fails, keep your data directory, quit the application, and reinstall a verified complete app from a trusted DMG; do not delete data to repair the executable.

## Build on macOS

Install Xcode 26+ (or Command Line Tools with the macOS 26+ SDK) and Rust 1.90.0. Select that Xcode with `DEVELOPER_DIR` or `xcode-select`; the SDK supplies Liquid Glass declarations while the application deployment target remains macOS 13.0. The native build has no Node.js, npm, browser or Python runtime requirement after installation. Python 3 is used only for build tooling. The script builds both targets, makes the icon from the retained brand PNG, bundles notices, ad-hoc signs, verifies the app, and creates the updater tarball and DMG. It refuses to overwrite an existing output directory.

```bash
rustup toolchain install 1.90.0 --profile minimal
rustup target add --toolchain 1.90.0 aarch64-apple-darwin x86_64-apple-darwin

# Choose cache locations outside any synchronized project directory.
export CARGO_TARGET_DIR="$HOME/Library/Caches/PortCanvas/build"
export WCS_TEST_ROOT="$HOME/Library/Caches/PortCanvas/tests"
export WCS_DATA_DIR="$WCS_TEST_ROOT/gui-data"
export TMPDIR="$HOME/Library/Caches/PortCanvas/tmp/"
mkdir -p "$CARGO_TARGET_DIR" "$WCS_DATA_DIR" "$TMPDIR" "$HOME/Library/Caches/PortCanvas/packages"
export MACOSX_DEPLOYMENT_TARGET=13.0
export RUSTUP_TOOLCHAIN=1.90.0
export PORTCANVAS_TEST_KEYCHAIN_SERVICE="io.portcanvas.test.credentials.local-$(uuidgen)"
cargo fetch --manifest-path app/native-probe/Cargo.toml --locked --target aarch64-apple-darwin --target x86_64-apple-darwin
python3 -B app/tools/macos_test_keychain.py test --manifest-path app/core/Cargo.toml --locked --features downloads
python3 -B app/tools/macos_test_keychain.py test --manifest-path app/native-probe/Cargo.toml --locked
python3 -B -m unittest discover -s app/tests -p 'test_*.py'
python3 -B app/tools/macos_test_keychain.py cleanup
bash app/tools/build-macos.sh --output "$HOME/Library/Caches/PortCanvas/packages/native-0.6.1"
```

The test wrapper records exact executable-specific disposable Keychain services in `WCS_TEST_ROOT/test-keychain-items.tsv`, alongside the unit tests' random service ledger. Run `cleanup` even after a failed test: it validates every ownership record, removes only those exact services for the test key account, and verifies their absence. It does not enumerate Keychain entries or delete by prefix. Keep `WCS_TEST_ROOT` isolated for this run.

The new output contains `PortCanvas.app`, `PortCanvas_0.6.1_universal.dmg`, `PortCanvas_0.6.1_universal.app.tar.gz`, `SHA256SUMS` and `validation.json`. `Contents/Resources/BUILD-INFO.json` records source revision, whether the checkout has local modifications, build epoch, version and signing status. The packaging check requires both Mach-O slices, macOS 13.0 load commands, system-only dynamic dependencies, bundle metadata, icon, licenses and a valid ad-hoc signature; `hdiutil verify` checks the DMG. It does not launch the UI or connect remote devices.

Cargo lockfiles, Rust version and the deployment target define repeatable build inputs. Tar ownership, timestamps and entry order are normalized from `SOURCE_DATE_EPOCH` (default: source commit time), and archives contain only regular files/directories under one `PortCanvas.app` root. DMG filesystem metadata and host SDK/compiler differences mean the complete output is not claimed to be byte-for-byte reproducible. Mac CI runs the shared and native tests on `macos-15-intel` with its installed Xcode 26 SDK, then builds and verifies both CPU slices in one Universal package. This runner executes x86_64 tests and cross-compiles arm64; local Apple Silicon interaction evidence is recorded separately. Each run records its actual OS, architecture and SDK. Windows still uses `app/tools/build-native.sh` and its existing installer/CI.

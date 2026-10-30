# Windows native workbench and feasibility probe

An isolated v0.6.0 development application. It does not replace the current PortCanvas application. macOS adaptation is outside the current scope.

The window uses Slint/winit/FemtoVG and native OpenGL. The terminal uses `alacritty_terminal`; RFB uses `vnc-rs` over an owned loopback fixture; H.264 decoding and test-video generation use Windows Media Foundation. Native signature verification accepts the existing signer's format and requires its authenticated version field. There is no Tauri, WebView, noVNC or xterm dependency in this crate's active Windows build graph.

The crate now links [the shared business services](../core/README.md). With `WCS_TEST_ROOT` set, `PortCanvasNativeProbe.exe --core-smoke` constructs, reads and shuts down those services in an isolated data directory without opening a window or connecting a device. The graphical prototype still uses controlled local data; real-controller validation is scheduled after the development and local-regression flow.

## Native application

`PortCanvasNative.exe` is the default GUI binary. It provides saved device profiles, password/private-key SSH authentication, host identity confirmation, connection progress/cancellation/disconnect, explicit LAN discovery, language, appearance, desktop quality and runtime-source preferences, and the local log-size limit. Both the legacy and native adapters use `core::application` for profile actions and SSH trust policy. Connection details and errors come from the shared services; switching pages or appearance does not reconnect the device.

The default development data directory is `%LOCALAPPDATA%/PortCanvas/NativeDevelopment`. Set an absolute `WCS_DATA_DIR` to use isolated fixtures. It does not automatically import production profiles or migrate data. Saved credentials use the shared Windows protection mechanism. The window does not scan the LAN automatically; discovery starts only when Search is pressed. Do not point the development build at the sole copy of production data.

This is the first M3 increment. Log browsing, background/theme options, the remaining window interactions, and feature parity are still outstanding. Terminal, remote desktop, files, arena and media pages are not yet integrated into this application. The separate `PortCanvasNativeProbe.exe` remains available for controlled renderer/codec/input experiments.

For local SSH GUI checks, `cargo build --locked --example workbench_fixture` builds an owned loopback server. With an isolated `WCS_TEST_ROOT`, it writes `fixture-ready.json` containing its ephemeral port and data directory. Launch the application with `WCS_DATA_DIR` set to that directory. The fixture accepts only synthetic credentials, executes no commands, and exits after ten minutes or when `fixture-stop` appears. Its final report contains the successful authentication count. No private key is written to disk.

## Build and test

Windows x64, Rust 1.90+, the MSVC build environment and Windows system media codecs are required. From this directory in Git Bash:

```bash
mkdir -p target/local-tests
export WCS_TEST_ROOT="$(cygpath -w "$PWD/target/local-tests")"
cargo test --locked
cargo build --locked --features gui --release
target/release/PortCanvasNative.exe
```

To generate a disposable, silent 320×180 H.264 fixture and run the window:

```bash
target/release/PortCanvasNativeProbe.exe --media-fixture "$WCS_TEST_ROOT/synthetic.mp4"
export PORTCANVAS_PROBE_VIDEO="$WCS_TEST_ROOT/synthetic.mp4"
export PORTCANVAS_PROBE_REPORT="$WCS_TEST_ROOT/gui-report.json"
target/release/PortCanvasNativeProbe.exe
```

The fixture destination must not exist. The report is optional and contains local terminal text, delivered frame counts, native-hide observations and worker errors. Use synthetic text rather than secrets. The window echoes local input and never executes shell commands. Its desktop button connects only to a generated loopback server. Tab changes, native minimization/occlusion, stop and exit cancel owned workers; exit joins them. Images pass through bounded latest-frame slots. The render timer runs only while a stream is active. The native media probe uses RGB32 CPU decoding/conversion and GPU texture upload; it does not yet establish hardware-decoder or zero-copy behavior.

## Acceptance boundary

The separate M1 probe tests VT parsing/scrollback/selection, native text submission, a synthetic RFB client/server path, system H.264 video and signed-version checks. Direct terminal IME composition, live SSH PTY/tmux, all terminal modes, every RFB encoding/filter, real x11vnc latency, production clipboard behavior, audio/codecs, background editing/compositing, DPI/accessibility coverage and the actual installer/update migration remain outstanding. Input-queue saturation is not qualified for real remote use. Local loopback SSH/GUI checks do not establish real-controller compatibility or a performance result.

Slint 1.16.1 showed repeated missing-CJK-segmentation diagnostics in the tested Windows text path. This probe pins 1.15.1, whose text and controls were checked locally. This is a prototype choice, not a frozen final GUI decision; egui/eframe remains an alternative to compare when the remaining gates can be measured.

The public test vectors in `tests/fixtures/update.json` were produced with the existing Tauri CLI signer using disposable keys. They contain no private key. That legacy comparison tool is not part of this native crate's build or runtime. Signature compatibility does not prove cross-stack installer compatibility. Nothing in the probe downloads or executes updates.

## Licenses

PortCanvas probe code is MIT. Slint is used under its royalty-free desktop license; its required AboutSlint widget is reachable through the About page. See [the retained terms](LICENSE-Slint.md) and [dependency notes](THIRD-PARTY-NOTICES.md). Source and lockfile references do not replace the final distribution license audit. In particular, SVG tooling in the build graph includes `simplecss`; this is not a browser engine, but the final all-chain inventory must account for it rather than claiming that every CSS-related dependency has already disappeared.

# Windows native feasibility probe

An isolated v0.6.0 development probe. It does not replace the current PortCanvas application or read its profiles, credentials, settings or logs. macOS adaptation is outside the current scope.

The window uses Slint/winit/FemtoVG and native OpenGL. The terminal uses `alacritty_terminal`; RFB uses `vnc-rs` over an owned loopback fixture; H.264 decoding and test-video generation use Windows Media Foundation. Native signature verification accepts the existing signer's format and requires its authenticated version field. There is no Tauri, WebView, noVNC or xterm dependency in this crate's active Windows build graph.

## Build and test

Windows x64, Rust 1.90+, the MSVC build environment and Windows system media codecs are required. From this directory in Git Bash:

```bash
mkdir -p target/local-tests
export WCS_TEST_ROOT="$(cygpath -w "$PWD/target/local-tests")"
cargo test --locked
cargo build --locked --features gui --release
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

This is an experimental M1 probe, not feature parity or a performance result. It currently tests VT parsing/scrollback/selection, native text submission, a synthetic RFB client/server path, system H.264 video and signed-version checks. Direct terminal IME composition, live SSH PTY/tmux, all terminal modes, every RFB encoding/filter, real x11vnc latency, production clipboard behavior, audio/codecs, background editing/compositing, DPI/accessibility coverage and the actual installer/update migration remain outstanding. Input-queue saturation is not qualified for real remote use. No real device connection is exposed.

Slint 1.16.1 showed repeated missing-CJK-segmentation diagnostics in the tested Windows text path. This probe pins 1.15.1, whose text and controls were checked locally. This is a prototype choice, not a frozen final GUI decision; egui/eframe remains an alternative to compare when the remaining gates can be measured.

The public test vectors in `tests/fixtures/update.json` were produced with the existing Tauri CLI signer using disposable keys. They contain no private key. That legacy comparison tool is not part of this native crate's build or runtime. Signature compatibility does not prove cross-stack installer compatibility. Nothing in the probe downloads or executes updates.

## Licenses

PortCanvas probe code is MIT. Slint is used under its royalty-free desktop license; its required AboutSlint widget is reachable through the About page. See [the retained terms](LICENSE-Slint.md) and [dependency notes](THIRD-PARTY-NOTICES.md). Source and lockfile references do not replace the final distribution license audit. In particular, SVG tooling in the build graph includes `simplecss`; this is not a browser engine, but the final all-chain inventory must account for it rather than claiming that every CSS-related dependency has already disappeared.

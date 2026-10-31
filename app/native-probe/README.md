# Windows native workbench and feasibility probe

An isolated v0.6.0 development application. It does not replace the current PortCanvas application. macOS adaptation is outside the current scope.

The window uses Slint/winit/FemtoVG and native OpenGL. The terminal uses `alacritty_terminal`; RFB uses `vnc-rs` over an owned loopback fixture; H.264 decoding and test-video generation use Windows Media Foundation. Native signature verification accepts the existing signer's format and requires its authenticated version field. There is no Tauri, WebView, noVNC or xterm dependency in this crate's active Windows build graph.

The crate now links [the shared business services](../core/README.md). With `WCS_TEST_ROOT` set, `PortCanvasNativeProbe.exe --core-smoke` constructs, reads and shuts down those services in an isolated data directory without opening a window or connecting a device. The graphical prototype still uses controlled local data; real-controller validation is scheduled after the development and local-regression flow.

## Native application

`PortCanvasNative.exe` is the default GUI binary. It provides saved device profiles, password/private-key SSH authentication, host identity confirmation, connection progress/cancellation/disconnect, explicit LAN discovery, local log browsing/cleanup, language, appearance/accent palette, desktop quality and runtime-source preferences, and the local log-size limit. Both the legacy and native adapters use `core::application` for profile actions and SSH trust policy. Connection details and errors come from the shared services; switching pages or appearance does not reconnect the device.

The current workbench is a functional development UI. The final native interface will use v0.5.95's layout, navigation, interactions and visual style as its reference after the functional chain is integrated; effects still require performance validation.

The default development data directory is `%LOCALAPPDATA%/PortCanvas/NativeDevelopment`. Set an absolute `WCS_DATA_DIR` to use isolated fixtures. It does not automatically import production profiles or migrate data. Saved credentials use the shared Windows protection mechanism. The window does not scan the LAN automatically; discovery starts only when Search is pressed. Do not point the development build at the sole copy of production data.

The terminal page now uses the shared PTY/tmux pool and local log observer. Tabs keep independent VT grids, selections, command drafts, command history and log projects. Connect/resume and detach preserve the remote session; restarting or closing requires confirmation and uses the shared lifecycle. Direct input uses native committed text and IME preedit at the VT cursor. Ctrl+Shift+C copies the selection, Ctrl+V pastes, and Ctrl+Tab moves focus from remote input to the log-project field. Raw Tab goes to the PTY.

The command editor supports IME, selection/caret editing, expanded mode and up to 100 successfully submitted commands per tab in memory. In collapsed mode, Enter sends, Shift+Enter inserts a newline, and Up/Down browse history for a single-line draft, restoring the unsent draft when moving past the newest entry. Expanded mode uses Enter for newlines; Ctrl+Enter sends in either mode. Failed sends retain the draft.

Tab requests command/path candidates through the existing shared SSH completion service without writing to the PTY. The current shell word is replaced with quoted text, preserving the rest of the draft and UTF-8 selection-start offsets. Multiple matches can extend a common prefix; a truncated result does not. Up/Down or Tab/Shift+Tab select, Enter or a pointer chooses, and Escape cancels before exiting full screen. The candidate viewport shows four rows and scrolls to the keyboard selection. Editing, changing the completion position, losing focus, changing tabs/pages or hiding the window invalidates pending results. Requests are bound to the terminal ID and local request serial. No shell evaluation is performed by the native word scanner.

Output uses the shared 256-KiB-per-stream credit window, acknowledged after parsing, with coalesced UI wakes and 1,000 lines of scrollback. Only the visible, non-minimized terminal grid is rendered. Hidden tabs continue parsing and recording; wheel scrolling changes local history without sending mouse/arrow reports. PTY resizes are coalesced for 80 ms. The shared connection generation and terminal stream IDs reject stale input/output; queued final output can still be displayed after a terminal exits.

The log page uses the shared 200-record cursor pages and 256-KiB detail tail, with project/day/status filters, a virtualized list, read-only original output, completeness notices, selectable finished records and confirmed cleanup. Root changes preserve the previous files and invalidate old view requests. Only the selected running record is polled, and polling stops off-page, while minimized/occluded or during confirmation. Completed task handles are reaped as new log work starts. Opening the log root creates that directory if necessary; opening a selected record requires its directory to exist.

Blue/green/graphite accent palettes apply to navigation, selection and primary actions; other standard widgets retain their platform styling. F11 and the toolbar toggle full screen, and Escape exits it when no confirmation is open. Those application shortcuts are consumed before remote input. Background/media options, full DPI/accessibility coverage, the remaining window interactions, and feature parity are still outstanding. Remote desktop, files, arena and media pages are not yet integrated into this application. The separate `PortCanvasNativeProbe.exe` remains available for controlled renderer/codec/input experiments.

For local SSH GUI checks, `cargo build --locked --example workbench_fixture` builds an owned loopback server. With an isolated `WCS_TEST_ROOT`, it writes `fixture-ready.json` containing its ephemeral port and data directory. Launch the application with `WCS_DATA_DIR` set to that directory. The fixture accepts only synthetic credentials, executes no commands, and exits after ten minutes or when `fixture-stop` appears. Its final report contains the successful authentication count. No private key is written to disk.

`cargo build --locked --example logs_fixture` creates a separate local log fixture. It requires a fresh isolated `WCS_TEST_ROOT`, generates 208 records (including a live external writer, failed/interrupted records and long Unicode output), and writes its development data location to `logs-ready.json`. It executes no commands and exits after fifteen minutes or when `logs-stop` appears. The optional `PORTCANVAS_WORKBENCH_REPORT` contains page/detail-read counters and visibility observations for local lifecycle checks; it does not contain log text or credentials.

`cargo build --locked --example terminal_fixture` builds a loopback SSH protocol fixture for the terminal UI. A fresh isolated `WCS_TEST_ROOT` receives `terminal-ready.json` (data directory, ephemeral port and public fingerprint). It uses synthetic credentials, emulates environment/bootstrap/control/completion replies and echoes PTY bytes; it never invokes a shell or tmux. `fixture-lines` requests 1,400 synthetic lines and `fixture-modes` enables application-cursor/bracketed-paste/mouse modes. Completion fixtures include `py`, `中`, `single`, `many` (64 truncated matches) and `slow` (three-second reply). It exits after fifteen minutes or `terminal-stop`. Reports contain sizes, byte/query counts and owned-channel/session counters, not input payloads. This fixture tests the native/shared-service wiring, not a real tmux server or controller.

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

The separate M1 probe tests VT parsing/scrollback/selection, a synthetic RFB client/server path, system H.264 video and signed-version checks. The workbench adds locally verified direct IME composition, Unicode input, selection/clipboard, two SSH PTYs, command drafts, resize and shared lifecycle tests. All terminal modes/modified keys, queue saturation, screen-reader coverage, actual tmux/x11vnc behavior, real-device latency, production clipboard behavior, audio/codecs, background editing/compositing and the installer/update migration remain outstanding. Local loopback SSH/GUI checks do not establish real-controller compatibility or a performance result.

Slint 1.16.1 showed repeated missing-CJK-segmentation diagnostics in the tested Windows text path. This probe pins 1.15.1, whose text and controls were checked locally. This is a prototype choice, not a frozen final GUI decision; egui/eframe remains an alternative to compare when the remaining gates can be measured.

The public test vectors in `tests/fixtures/update.json` were produced with the existing Tauri CLI signer using disposable keys. They contain no private key. That legacy comparison tool is not part of this native crate's build or runtime. Signature compatibility does not prove cross-stack installer compatibility. Nothing in the probe downloads or executes updates.

## Licenses

PortCanvas probe code is MIT. Slint is used under its royalty-free desktop license; its required AboutSlint widget is reachable through the About page. See [the retained terms](LICENSE-Slint.md) and [dependency notes](THIRD-PARTY-NOTICES.md). Source and lockfile references do not replace the final distribution license audit. In particular, SVG tooling in the build graph includes `simplecss`; this is not a browser engine, but the final all-chain inventory must account for it rather than claiming that every CSS-related dependency has already disappeared.

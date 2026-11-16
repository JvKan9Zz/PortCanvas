<p align="center"><img src="assets/brand/portcanvas-readme.png" width="96" height="96" alt="PortCanvas Logo"></p>

# PortCanvas

[简体中文](README.md) | English

A native Windows workspace for remote Linux devices. [**Download v0.6.1**](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.6.1).

The Rust/Slint workbench restores v0.5.95's page structure and icons, with native VT/RFB and Windows media playback. It provides SSH discovery and authentication, window-contained remote desktop expansion, dual-pane SFTP, persistent tmux terminals, local logs, camera preview and device metrics. Glass quality and opacity are independent; Full is the default, and a lower-quality suggestion requires the user's choice.

[Download the Windows x64 installer](https://github.com/JvKan9Zz/PortCanvas/releases/download/v0.6.1/PortCanvas_0.6.1_x64-setup.exe), then open the PortCanvas desktop shortcut. Stable data lives in `%LOCALAPPDATA%/PortCanvas/Stable`, separately from the native Beta. Review and confirm import of legacy or Beta data from the Devices page; the source remains intact. Keep portable `data/` folders. In-app updates use the stable channel. Terminals support scrollable tabs, horizontal long-line viewing, vertical history, and per-device program launch presets. File and log panes are resizable, and a single selected regular file can have its full path copied.

[Windows v0.5.95](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.95) · [macOS v0.5.95](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.95) · [Usage](docs/usage.md) · [Pose protocol](docs/pose-protocol.md)

Current native development targets Windows x64 only. The published macOS v0.5.95 Universal DMG remains a fixed historical build with ad-hoc signing, no Apple notarization and manual updates. Further Mac adaptation is handled on request.

See [app](app/README.md) for the current native source, build and installer. The installed Windows application needs no Node.js, Rust or Python runtime. Remote features use SSH and the controller's Python 3, tmux, SFTP, x11vnc and camera tools. Component preparation currently targets the reference Ubuntu 20.04 ARM64 controller and uses an isolated user cache.

Camera preview is off by default and is released when leaving the page, minimizing, disconnecting or exiting. Restoring the window leaves it off. Frame delivery rates are not physical latency measurements. Pose coordinates remain unavailable without a real producer.

Project code is [MIT](LICENSE); dependencies, fonts and Slint retain their own terms in [the native notices](app/native-probe/THIRD-PARTY-NOTICES.md). See [security notes](docs/security.md). Historical releases and source remain in Git tags and [Releases](https://github.com/JvKan9Zz/PortCanvas/releases).

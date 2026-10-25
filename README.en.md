<p align="center"><img src="assets/brand/portcanvas-readme.png" width="96" height="96" alt="PortCanvas Logo"></p>

# PortCanvas

[简体中文](README.md) | English

A unified remote device workspace · **Demo V0.5.3**

This release adds connection-stage diagnostics and in-app updates with 32 parallel download streams, signature verification and automatic restart.

Connect to Linux devices, control their desktops, transfer files, run terminals and read local execution logs from one Windows client.

| Feature | Current capabilities |
| --- | --- |
| Devices | SSH password/private-key authentication, optional encrypted credential storage and host-key confirmation |
| Desktop and files | X11 remote desktop, dual-pane SFTP, drive selection, drag-and-drop transfers, progress and cancellation |
| Terminals and logs | Multiple terminals, restart/close, reconnectable sessions and command completion; local logs grouped by project/date |
| Arena | On-demand camera preview at target rates of 8/15/20/30 FPS, device metrics and 60-second history, full-field metric coordinates and a position input interface |
| Settings | Chinese/English, light blue/green/gray themes and in-app updates for installed and portable builds |

No localization producer is connected yet. Position readings stay unavailable rather than showing simulated live data. The camera starts off and stops on minimize, page departure or exit; restoring the workspace does not reopen it. Native MJPEG avoids board-side re-encoding, at the cost of more bandwidth and some host overhead. Results depend on the device and network.

## Download and usage

[Latest release](https://github.com/JvKan9Zz/PortCanvas/releases/latest) · [Usage guide (Chinese)](docs/usage.md) · [Position protocol (Chinese)](docs/pose-protocol.md)

Windows x64 installer and portable packages include SHA256SUMS.txt. The client uses the system WebView2 runtime; Node.js, Rust and Python are not required on the Windows computer at runtime. Installer mode preserves existing settings; portable mode uses a local data/ directory. Personal settings, private keys, execution logs and camera photos are excluded from the repository and packages.

## Source

| Directory | Purpose |
| --- | --- |
| [app](app) | Tauri 2 + Rust + WebView2 application and [build instructions](app/README.md) |
| [assets](assets) | Current brand and arena source assets |
| [docs](docs) | Current usage and position-interface documentation |

main contains only the current application and necessary documentation. Older versions and development records remain available in [Releases](https://github.com/JvKan9Zz/PortCanvas/releases) and [Git history](https://github.com/JvKan9Zz/PortCanvas/commits/main/). History is not rewritten.

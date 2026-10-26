<p align="center"><img src="assets/brand/portcanvas-readme.png" width="96" height="96" alt="PortCanvas Logo"></p>

# PortCanvas

[简体中文](README.md) | English

A unified remote device workspace · **Demo V0.5.7**

This release checks and prepares missing remote desktop, terminal, file and camera components over the existing SSH connection. Local logs support an optional storage limit with oldest-finished-record cleanup. The Windows edition retains in-app updates.

Connect to Linux devices, control their desktops, transfer files, run terminals and read local execution logs from one desktop client.

| Feature | Current capabilities |
| --- | --- |
| Devices | SSH password/private-key authentication, optional encrypted credential storage and host-key confirmation |
| Desktop and files | X11 remote desktop with three picture modes and automatic pause/resume; dual-pane SFTP, drive selection, drag-and-drop transfers, remote deletion, progress and cancellation |
| Terminals and logs | Multiple terminals, restart/close, reconnectable sessions and an expandable command box with completion; local logs grouped by project/date |
| Arena | On-demand camera preview at target rates of 8/15/20/30 FPS, device metrics and 60-second history, full-field metric coordinates and a position input interface |
| Settings | Chinese/English, light blue/green/gray themes and in-app updates for the Windows installer edition |

No localization producer is connected yet. Position readings stay unavailable rather than showing simulated live data. The camera starts off and stops on minimize, page departure or exit; restoring the workspace does not reopen it. Native MJPEG avoids board-side re-encoding, at the cost of more bandwidth and some host overhead. Results depend on the device and network.

First-time component preparation requires the Linux device to reach its configured package repositories. Packages are extracted into a private user cache, without system installation or SDK changes. Linux, Python 3 and a logged-in X11 desktop remain prerequisites. Existing components are reused; prepared devices can continue working over the local network.

## Download and usage

[Latest Windows installer](https://github.com/JvKan9Zz/PortCanvas/releases/latest) · [macOS V0.5.6](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.6) · [Usage guide (Chinese)](docs/usage.md) · [Position protocol (Chinese)](docs/pose-protocol.md)

The Windows x64 installer lets you choose the installation folder. In-app updates retain that folder and existing configuration. The universal DMG targets macOS 13+ and includes Intel and Apple Silicon code. Demo V0.5.6 is currently the only macOS compatibility release. Further macOS adaptation may wait until the stable version is released; later Windows demos will not automatically receive a Mac build. Both editions use the operating system's webview and need no Node.js, Rust or Python at runtime.

The Mac package uses an ad-hoc signature and is not notarized by Apple. First launch may require manual approval in macOS Privacy & Security. Saved credentials use Windows DPAPI or macOS Keychain and cannot be decrypted directly on the other operating system.

Portable builds, wrapper ZIPs, standalone signatures and checksum lists are no longer published. Besides installers and GitHub's source archives, releases retain only the latest.json needed by the Windows in-app updater; its signature is embedded in that manifest.

## Source

| Directory | Purpose |
| --- | --- |
| [app](app) | Tauri 2 + Rust + WebView2 application and [build instructions](app/README.md) |
| [assets](assets) | Current brand and arena source assets |
| [docs](docs) | Current usage and position-interface documentation |

main contains only the current application and necessary documentation. Older versions and development records remain available in [Releases](https://github.com/JvKan9Zz/PortCanvas/releases) and [Git history](https://github.com/JvKan9Zz/PortCanvas/commits/main/). History is not rewritten.

## License

PortCanvas project code is licensed under [MIT](LICENSE). Third-party components retain their own licenses; see [third-party notices](app/THIRD-PARTY-NOTICES.txt).

See [security notes](docs/security.md) for cryptographic dependency status and current security boundaries.

<p align="center"><img src="assets/brand/portcanvas-readme.png" width="96" height="96" alt="PortCanvas Logo"></p>

# PortCanvas

[简体中文](README.md) | English

A unified remote device workspace · **Demo V0.5.9**

This release adds startup LAN SSH discovery and image/video backgrounds with volume control. It removes the image size cap and fixes large images appearing only in the preview. Backgrounds stream from local files, and closed previews release their decoder resources. The liquid-glass workspace, light/dark/system appearance and adjustable opacity remain available. Missing remote components download and cache on the PC by default, then transfer over SSH; board-side downloading is also selectable. New log configurations default to a 1024 MiB limit.

Connect to Linux devices, control their desktops, transfer files, run terminals and read local execution logs from one desktop client.

| Feature | Current capabilities |
| --- | --- |
| Devices | Startup LAN SSH discovery; user-supplied device names and accounts; SSH password/private-key authentication, optional encrypted credential storage and host-key confirmation |
| Desktop and files | X11 remote desktop with three picture modes and automatic pause/resume; dual-pane SFTP, drive selection, drag-and-drop transfers, remote deletion, progress and cancellation |
| Terminals and logs | Multiple terminals, restart/close, reconnectable sessions and an expandable command box with completion; local logs grouped by project/date |
| Arena | On-demand camera preview at target rates of 8/15/20/30 FPS, device metrics and 60-second history, full-field metric coordinates and a position input interface |
| Settings | Chinese/English, three accent colors, light/dark/system appearance, custom background color/image/video, video volume, glass opacity and grouped settings with Windows in-app updates |

No localization producer is connected yet. Position readings stay unavailable rather than showing simulated live data. The camera starts off and stops on minimize, page departure or exit; restoring the workspace does not reopen it. Native MJPEG avoids board-side re-encoding, at the cost of more bandwidth and some host overhead. Results depend on the device and network.

By default, the app downloads and caches missing components on the PC, then transfers them over SSH; the controller does not need Internet access to download packages. Separate progress indicators cover downloading, transfer and preparation. Downloads use up to 32 connections, falling back to one when ranges are unsupported. Automatic downloads currently target Ubuntu 20.04 ARM64 controllers with usable APT package indexes, Python 3 and dpkg. A logged-in X11 desktop is still required. Existing components and PC caches are reused; packages remain in the SSH user's private cache without system installation or SDK changes.

## Download and usage

[Windows V0.5.9](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.9) · [macOS V0.5.6](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.6) · [Usage guide (Chinese)](docs/usage.md) · [Position protocol (Chinese)](docs/pose-protocol.md)

Installed Windows editions can check for updates under Settings → About, or use the new installer. Version 0.5.8 can update directly to 0.5.9. Background images and videos can be cropped before applying. Settings → Storage selects the runtime download source; an operation already in progress keeps the source it started with.

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

The UI bundles PingFang fonts with redistribution permission confirmed by the owner. Terminal output uses a monospace font with PingFang for Chinese. The header wordmark alone uses Macintosh Skeletal Filled Regular with separately confirmed redistribution permission. Fonts retain their separate license and are not covered by the project MIT license.

See [security notes](docs/security.md) for cryptographic dependency status and current security boundaries.

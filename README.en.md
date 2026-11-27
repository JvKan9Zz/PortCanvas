<div align="center">

<img src="assets/brand/portcanvas-readme.png" width="92" height="92" alt="PortCanvas Logo">

<h1>PortCanvas</h1>

<p><strong>A fast, native Liquid Glass workbench for remote Linux devices</strong></p>

<p>
  Built with Rust &amp; Slint — combining SSH device management, remote desktop, dual-pane SFTP,<br>
  persistent multi-tab terminals, live hardware telemetry, and a signed plugin ecosystem with zero WebView overhead.
</p>

<p>
  <a href="https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.7.1"><img src="https://img.shields.io/badge/Windows-v0.7.1-2563eb?style=flat-square" alt="Windows v0.7.1"></a>
  <a href="https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.7.1"><img src="https://img.shields.io/badge/macOS_13+-v0.7.1_Universal-0f172a?style=flat-square" alt="macOS v0.7.1 Universal"></a>
  <img src="https://img.shields.io/badge/Built_with-Rust_%2B_Slint-ea580c?style=flat-square" alt="Rust + Slint">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-16a34a?style=flat-square" alt="MIT License"></a>
</p>

<p>
  <a href="README.md">简体中文</a> · <strong>English</strong>
  &nbsp;&nbsp;|&nbsp;&nbsp;
  <a href="https://github.com/JvKan9Zz/PortCanvas/releases/download/v0.7.1/PortCanvas_0.7.1_x64-setup.exe"><strong>Download Windows v0.7.1</strong></a>
  · <a href="https://github.com/JvKan9Zz/PortCanvas/releases/download/v0.7.1/PortCanvas_0.7.1_universal.dmg">macOS v0.7.1 DMG</a>
  · <a href="docs/usage.md">Usage Guide</a>
  · <a href="https://github.com/JvKan9Zz/PortCanvas/releases">Releases</a>
</p>

<br>

<img src="assets/preview/workbench-light.png" width="100%" alt="PortCanvas v0.7.1 Frameless Liquid Glass Workbench">

</div>

---

## Why PortCanvas

- **100% Native Performance**: Powered by Rust, Slint (FemtoVG), native VT terminal emulation, native RFB remote desktop, and system media playback. No Electron, no WebView, and zero runtime prerequisites after installation.
- **Frameless Liquid Glass Design**: Windows uses a full-width floating glass header with native Win32 window chrome (drag, edge/corner resize, Snap, and auto-hide taskbar reveal), while macOS integrates native system materials. Glass quality (Full by default) and opacity are independently configurable with color, cropped image, or video backgrounds.
- **Dedicated Device Monitor**: Promoted into a standalone Monitor page delivering 60-second CPU/memory curves, per-core load & clock speed, storage/disk usage across mount points, per-interface network rates & RTT, multi-zone thermals, and system overview.
- **Signed Extensible Plugin System**: Install official `.pcplugin` packages directly from the HTTPS + Minisign-verified catalog or import offline archives. Installed plugins live cleanly in `<install>/plugins/` on Windows (and `<data>/plugins/` on macOS).
- **Uninterrupted Multi-Tool Workflow**: Keep one persistent `tmux`-backed terminal drawer docked on the right while moving seamlessly across Remote Desktop, SFTP Files, Local Logs, Device Monitor, and Plugin workspaces.

---

## Features

| Area | Highlights |
| --- | --- |
| **Devices** | LAN SSH discovery, password or private-key auth, host fingerprint verification, OS-encrypted optional credential storage, staged connection progress, and `.portcanvas` config export/import with automatic backup |
| **Remote Desktop** | Native X11/RFB transport, three quality presets, Fit / `1:1` scaling, read-only mode, clipboard sync, and smooth in-window expansion |
| **Files (SFTP)** | Resizable dual-pane browser, multi-select drag-and-drop, recursive transfers, overwrite confirmation, bandwidth throttling, cancellation, and single-file full-path copy |
| **Terminals & Logs** | Scrollable multi-tab persistent `tmux` terminals, IME support, command drafts/history/completion, per-device program launch presets, and filterable local run logs with storage caps |
| **Device Monitor** | Live 2-second telemetry (auto-stopped on leave, hide, or disconnect): 4 KPI cards, 60s dual trend chart, per-core CPU & frequency, disk storage breakdown, per-NIC throughput & RTT, thermals, and OS/kernel facts |
| **Plugins** | Auto-refreshing downloadable catalog with Minisign verification, offline `.pcplugin` import, atomic install/rollback, and clean stream teardown on uninstall; ships with **Arena Monitor (`v0.7.0`)** & **Vision Studio (`v0.1.0`)** |
| **Appearance** | Instant Chinese/English switching, Light / Dark / System themes, accent palettes, custom color/image/video backgrounds, and real-time Liquid Glass tuning |

---

## Official Plugins

Open **Plugins → Available** inside PortCanvas to download signed official plugins directly, or import `.pcplugin` bundles offline:

| Plugin | Version | Description |
| --- | --- | --- |
| **Arena Monitor** (`arena-monitor`) | `v0.7.0` | 3.8m combat arena map, live robot pose telemetry (coordinates, heading, confidence, latency via [pose protocol](docs/pose-protocol.md)), and on-demand remote camera preview |
| **Vision Studio** (`vision-studio`) | `v0.1.0` | Live OpenCV camera lab with 7 hot-swappable pipelines (raw/flip, grayscale & Otsu threshold, Canny/Sobel/Shi-Tomasi corners, HSV color & bottom-ROI line-following, Hough circles & polygons, native AprilTag tracking, Haar face detection), plus **board camera companion watch** (real-time `/dev/video*` process inspection, X11 OpenCV window capture, and `/dev/shm/portcanvas_cam.jpg` headless shared-frame streaming) |

---

## Preview

<table>
  <tr>
    <td width="50%">
      <img src="assets/preview/monitor-light.jpg" alt="Standalone Device Performance Monitor">
      <p align="center"><sub>Standalone Device Performance Monitor (Trends, Per-Core, Storage, Network &amp; System)</sub></p>
    </td>
    <td width="50%">
      <img src="assets/preview/plugins-light.png" alt="Downloadable Plugins Marketplace">
      <p align="center"><sub>Signed Downloadable Plugins (Arena Monitor &amp; Vision Studio)</sub></p>
    </td>
  </tr>
  <tr>
    <td width="50%">
      <img src="assets/preview/vision-studio.jpg" alt="Vision Studio Live OpenCV Lab">
      <p align="center"><sub>Vision Studio Plugin: Live OpenCV Tuning &amp; Board Camera Companion Watch</sub></p>
    </td>
    <td width="50%">
      <img src="assets/preview/desktop-terminal.png" alt="Remote Desktop and Shared Terminal Drawer">
      <p align="center"><sub>Remote Desktop with Resizable Shared Terminal Drawer</sub></p>
    </td>
  </tr>
</table>

---

## Download & Install

| Platform | Latest | Package | Notes |
| --- | --- | --- | --- |
| **Windows x64** | **v0.7.1** | [**PortCanvas_0.7.1_x64-setup.exe**](https://github.com/JvKan9Zz/PortCanvas/releases/download/v0.7.1/PortCanvas_0.7.1_x64-setup.exe) | Stable channel with signed in-app updates; fresh installs store data in `<install>/data/`, plugins in `<install>/plugins/`, and silent 10 MiB diagnostic logs in `<install>/debuglogs/`; upgrades keep existing data |
| **macOS 13.0+** | **v0.7.1** | [**PortCanvas_0.7.1_universal.dmg**](https://github.com/JvKan9Zz/PortCanvas/releases/download/v0.7.1/PortCanvas_0.7.1_universal.dmg) | Apple Silicon &amp; Intel Universal build, ad-hoc signed for manual install ([SHA-256](https://github.com/JvKan9Zz/PortCanvas/releases/download/v0.7.1/PortCanvas_0.7.1_universal.dmg.sha256)); see [Mac guide](docs/macos.md) |

<details>
<summary><strong>Data migration, remote dependencies &amp; archive links</strong></summary>

- **Credentials & Migration**: Saved credentials stay encrypted on the local machine via Windows DPAPI or macOS Keychain and are never exported inside `.portcanvas` bundles. Legacy data and `.portcanvas` bundles can be previewed and imported from the Devices page with automatic backup.
- **Updates, Diagnostics & Plugin Trust**: Windows stable builds support Minisign-verified in-app updates and one-click debug log export ("Report Issue") via Settings → About. Production builds enforce HTTPS and Minisign verification for the plugin catalog.
- **Remote Linux Tools**: Reuses existing SSH, Python 3, `tmux`, SFTP, `x11vnc`, and FFmpeg/OpenCV on the target device. Optional component preparation targets Ubuntu 20.04 ARM64 controllers inside an isolated user cache without modifying system SDKs.
- **Previous Releases**: [v0.7.0](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.7.0) · [v0.6.4](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.6.4) · [v0.6.3](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.6.3) · [v0.6.2](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.6.2) · [All Releases](https://github.com/JvKan9Zz/PortCanvas/releases)

</details>

---

## Build from Source & Repository Map

Requires Rust 1.90+, Git Bash, and Visual Studio C++ Build Tools (Windows) or Xcode (macOS):

```bash
# Windows x64 static-CRT release build
cd app && bash tools/build-native.sh
```

| Path | Description |
| --- | --- |
| [app/](app/README.md) | Native Rust / Slint workspace (`core` services + `native-probe` workbench), tests & packaging tools |
| [plugins/](plugins) | Official plugin sources (`arena-monitor`, `vision-studio`) and signed `catalog.json` |
| [assets/](assets) | Brand artwork & README UI previews |
| [docs/usage.md](docs/usage.md) | Full user guide (Chinese) |
| [docs/macos.md](docs/macos.md) | macOS Universal build & installation guide |
| [docs/pose-protocol.md](docs/pose-protocol.md) | External arena pose protocol |
| [docs/security.md](docs/security.md) | Authentication, encryption, signed updates & data boundaries |

---

## License

Released under the [MIT License](LICENSE). Third-party crates, fonts, and Slint remain under their respective licenses; see [THIRD-PARTY-NOTICES.md](app/native-probe/THIRD-PARTY-NOTICES.md).

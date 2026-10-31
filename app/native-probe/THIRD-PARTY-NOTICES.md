# Native probe dependency notes

Direct dependencies retain their upstream licenses. Slint uses the retained royalty-free terms and the AboutSlint widget. Remaining transitive notices and distribution obligations must be collected before a production native release.

| Component | Upstream license |
| --- | --- |
| slint-1.15.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 |
| alacritty_terminal-0.26.0 | Apache-2.0 |
| vnc-rs-0.6.0 | MIT OR Apache-2.0 |
| image-0.25.10 | MIT OR Apache-2.0 |
| arboard-3.7.0 | MIT OR Apache-2.0 |
| minisign-verify-0.3.0 | MIT |
| windows-0.61.3 | MIT OR Apache-2.0 |
| rfd-0.15.4 | MIT |
| chrono-0.4.45 | MIT OR Apache-2.0 |

The native workbench uses the shared core's log format and services. Its local test examples also use uuid (Apache-2.0 OR MIT) and the existing russh fixture dependencies. These are development fixtures, not bundled services.

The retained [vnc-rs source and patch record](../vendor/vnc-rs/UPSTREAM.md) preserve the upstream [MIT](../vendor/vnc-rs/LICENSE-MIT) and [Apache-2.0](../vendor/vnc-rs/LICENSE-APACHE) licenses. The patch adds optional quality/frame boundaries, Latin-1 clipboard handling and a local input-write barrier.

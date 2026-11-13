# Security notes

SSH host keys are checked before authentication. An unknown host requires confirmation; a changed fingerprint is rejected. The current Windows native preview protects saved credentials with the current user's DPAPI. The historical macOS v0.5.95 client uses a Keychain-protected encryption key. Private-key files remain on the user's computer.

Preview updates use the built-in public key, HTTPS and signed-version verification on a separate prerelease channel; the stable latest.json channel is not changed. The Windows validation workflow neither installs an update nor accesses signing keys or real devices.

## Cryptographic dependency status

Cargo.lock uses crypto-bigint 0.7.9, a non-yanked patch release in the 0.7 series covering the fixes for [RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html). The advisory's minimum patched release, 0.7.6, has been yanked; it is not the version locked by this project.

The RSA dependency still matches [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html), for which upstream lists no patched version. PortCanvas uses RSA for SSH authentication signatures and host-signature verification, not as a network RSA decryption service. No exploit of this client has been demonstrated; this call-path distinction is not a claim that the dependency is free of side channels. Existing RSA key support is retained. Prefer Ed25519 keys for new setups and continue tracking upstream fixes.

Other Cargo.lock maintenance advisories include platform-specific packages. A lockfile match alone does not establish that the affected function is present in the Windows application. Dependency updates must retain the project's build and authentication checks.


## 局域网发现与本地背景

局域网发现仅在活动 IPv4 私有/链路本地网段读取 SSH 服务标识，未认证发现结果不视为受信设备。设备名称、账户、凭据由用户提供，主机密钥检查沿用原认证流程。

原生背景从确认后的本地受管文件或导入草稿直接解码，不创建媒体 HTTP 服务。桌面使用拥有明确生命周期的 SSH/RFB 流，相机通过 SSH 提供最新 JPEG 给原生解码器；不建立 WebSocket 或 HTTP 呈现桥接。源媒体不会上传外部服务，取消导入保留原件；应用缓存与原件分别管理。

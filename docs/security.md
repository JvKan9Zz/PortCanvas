# Security notes

SSH host keys are checked before authentication. An unknown host requires confirmation; a changed fingerprint is rejected. Windows protects saved credentials with the current user's DPAPI. macOS uses a Keychain-protected encryption key and preserves the existing same-account credential identity. Private-key files remain on the user's computer.

Native updates retain the built-in publisher public key, HTTPS transport and the existing Minisign signature with an authenticated application version. Payload tampering, downgrades and manifest version relabeling are rejected. Stable and Beta select their own channels. macOS selects only `darwin-aarch64` or `darwin-x86_64`; a manifest containing only Windows entries is not a Mac update.

Mac updates accept a `.app.tar.gz` with one `PortCanvas.app` root. Extraction rejects traversal, links, special/privileged files, duplicate files and excessive expanded sizes. Before executing a replacement, the updater verifies its bundle ID (`com.portcanvas.desktop`), signed version, native Mach-O architecture and the full macOS code signature. Its signing team must match the installed application; an ad-hoc installation accepts only an ad-hoc replacement. Distribution signing and Apple notarization are separate from the publisher's update signature. The PR test package is ad-hoc signed and is not notarized.

A separate copy of the running executable acts as the update helper in an owner-only directory beside the installed app. It verifies the on-disk signed archive independently, waits for the old PID to exit, and exchanges the complete bundles atomically on the same filesystem. The replacement must acknowledge its first rendered frame; startup failure or timeout stops the new process and swaps the original bundle back before restarting it. Successful launch removes the old code and the private download stage. User configuration, credentials, media and logs are not deleted. Updating requires an ordinary writable application location and a removable original bundle; the helper never requests elevation. A read-only DMG, a linked application path, locked files or an application owned by another account require manual installation.

Debug-only updater tests use disposable keys and isolated application/data trees. Release builds pin the production public key in both the application and helper; no replacement production key or official Mac release is created by the test/build workflow.

## Cryptographic dependency status

Cargo.lock uses crypto-bigint 0.7.9, a non-yanked patch release in the 0.7 series covering the fixes for [RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html). The advisory's minimum patched release, 0.7.6, has been yanked; it is not the version locked by this project.

The RSA dependency still matches [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html), for which upstream lists no patched version. PortCanvas uses RSA for SSH authentication signatures and host-signature verification, not as a network RSA decryption service. No exploit of this client has been demonstrated; this call-path distinction is not a claim that the dependency is free of side channels. Existing RSA key support is retained. Prefer Ed25519 keys for new setups and continue tracking upstream fixes.

Other Cargo.lock maintenance advisories include platform-specific packages. A lockfile match alone does not establish that the affected function is present in the Windows application. Dependency updates must retain the project's build and authentication checks.


## 局域网发现与本地背景

局域网发现仅在活动 IPv4 私有/链路本地网段读取 SSH 服务标识，未认证发现结果不视为受信设备。设备名称、账户、凭据由用户提供，主机密钥检查沿用原认证流程。

原生背景从确认后的本地受管文件或导入草稿直接解码，不创建媒体 HTTP 服务。桌面使用拥有明确生命周期的 SSH/RFB 流，相机通过 SSH 提供最新 JPEG 给原生解码器；不建立 WebSocket 或 HTTP 呈现桥接。源媒体不会上传外部服务，取消导入保留原件；应用缓存与原件分别管理。

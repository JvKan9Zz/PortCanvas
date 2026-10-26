# Security notes

SSH host keys are checked before authentication. An unknown host requires confirmation; a changed fingerprint is rejected. Saved credentials use the current user's DPAPI on Windows or a Keychain-protected encryption key on macOS. Private-key files remain on the user's computer.

Updates retain the built-in public key, HTTPS endpoint and signed-version verification. The Windows validation workflow neither installs an update nor accesses signing keys or real devices.

## Cryptographic dependency status

Cargo.lock uses crypto-bigint 0.7.9, a non-yanked patch release in the 0.7 series covering the fixes for [RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html). The advisory's minimum patched release, 0.7.6, has been yanked; it is not the version locked by this project.

The RSA dependency still matches [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html), for which upstream lists no patched version. PortCanvas uses RSA for SSH authentication signatures and host-signature verification, not as a network RSA decryption service. No exploit of this client has been demonstrated; this call-path distinction is not a claim that the dependency is free of side channels. Existing RSA key support is retained. Prefer Ed25519 keys for new setups and continue tracking upstream fixes.

Other Cargo.lock maintenance advisories include platform-specific packages. A lockfile match alone does not establish that the affected function is present in the Windows application. Dependency updates must retain the project's build and authentication checks.

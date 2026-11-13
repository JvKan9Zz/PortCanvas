# Shared PortCanvas services

`portcanvas-core` contains the existing SSH/session, terminal/tmux, SFTP, logs, profiles/credentials, preferences, discovery, camera/pose and environment-preparation services. The native workbench links this crate directly; protocol lifecycle regressions live with their owning services.

This crate has no Tauri dependency or build script. The optional `downloads` feature enables the existing HTTPS range transport and PC runtime cache; it does not enable a desktop UI. The native updater reuses this transport and performs version-bound signature verification at its application boundary.

`services::Services::new` accepts explicit data, document and local-file directories, portable mode, an event sink and the optional terminal namespace. It owns service construction, disconnect invalidation and shutdown. Its disconnect observer holds a weak service reference, so a UI callback does not become a required lifetime owner. `shutdown` cancels owned activity and closes device/log resources. Window handles, platform directory discovery, confirmations/file dialogs, and update installation remain caller responsibilities. Desktop, camera and background presentation no longer expose a browser HTTP/WebSocket bridge.

Existing JSON fields, DPAPI identity context, tmux names, logging protocols, cancellation and timeout behavior are retained. Ordinary writes to preferences, device profiles, log settings and the terminal catalog preserve opaque extension fields from the existing file. Device/tab metadata follows stable IDs rather than row positions; intentional removals stay removed. Opaque fields are not added to the public typed models. Persisted missing, duplicate or invalid device IDs are rejected without generating replacement identities; new-profile ID generation remains unchanged. Explicit repair of invalid log settings keeps its original-file backup.

This does not constitute a completed data/installer migration. Native import supports devices, logs, settings, terminals and referenced managed media; snapshot, conflict, backup and rollback checks remain in force. Production-path selection, native installation/update compatibility and complete rollback acceptance remain migration work. The shared library version is separate from the native application version.

`application` provides UI-independent save/remove/connect/disconnect actions. A caller supplies an asynchronous `HostConfirmation` for unknown SSH keys; known-key matching, changed-key rejection, credential resolution and the post-confirmation cancellation check stay in the shared core. The loopback integration tests exercise these rules without a window or device. The caller confirms profile removal before calling the shared removal action.

## Windows tests

Use Rust 1.90+ and the MSVC environment. From this directory in Git Bash:

```bash
mkdir -p target/local-tests
export WCS_TEST_ROOT="$(cygpath -w "$PWD/target/local-tests")"
cargo test --locked --features downloads
cargo tree --locked --features downloads --target x86_64-pc-windows-msvc -e normal,build
```

Python helper tests remain in `app/tests`; their input scripts and Rust fixtures now live here. The shared English catalog is in `resources/en.json` and is used at native display boundaries. Real-controller, physical latency and broader platform acceptance are recorded separately from these local checks.

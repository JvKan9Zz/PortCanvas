# Shared PortCanvas services

`portcanvas-core` contains the existing SSH/session, terminal/tmux, SFTP, logs, profiles/credentials, preferences, discovery, camera/pose and environment-preparation services. The legacy `studio_core` library re-exports these modules so existing adapter calls and integration tests keep their paths. The native probe links this same crate.

This crate has no Tauri dependency or build script. The optional `downloads` feature enables the existing HTTPS range transport and PC runtime cache; it does not enable a desktop UI. The transport is shared with the legacy updater, whose installer and signature adapter remain in `src-tauri`.

`services::Services::new` accepts explicit data, document and local-file directories, portable mode, an event sink and the optional terminal namespace. It owns service construction, disconnect invalidation and shutdown. Its disconnect observer holds a weak service reference, so a UI callback does not become a required lifetime owner. `shutdown` cancels owned activity and closes device/log resources. Window handles, platform directory discovery, confirmations/file dialogs, and update installation remain caller responsibilities. The legacy background HTTP bridge is initialized by its adapter rather than every native service caller.

Existing JSON fields, DPAPI identity context, tmux names, logging protocols, cancellation and timeout behavior are retained. Moving the crate does not constitute a new data migration; unknown-field preservation remains a separate migration requirement. The crate version remains aligned with the legacy 0.5.95 metadata until the application version is changed deliberately.

## Windows tests

Use Rust 1.90+ and the MSVC environment. From this directory in Git Bash:

```bash
mkdir -p target/local-tests
export WCS_TEST_ROOT="$(cygpath -w "$PWD/target/local-tests")"
cargo test --locked --features downloads --lib
cargo tree --locked --features downloads --target x86_64-pc-windows-msvc -e normal,build
```

Python helper tests remain in `app/tests`; their input scripts and Rust fixtures now live here. The shared English catalog is in `resources/en.json` and is used by both Rust and the legacy frontend. Real-controller validation is deferred until the native development/integration/local regression flow is complete, as requested by the user.

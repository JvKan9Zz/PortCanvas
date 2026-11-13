#!/usr/bin/env bash
set -euo pipefail

app_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# Use one CRT choice for the application and target libraries. The explicit
# target leaves host build scripts and procedural macros unchanged.
CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="${CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS-} -C target-feature=+crt-static" \
  cargo build --manifest-path "$app_root/native-probe/Cargo.toml" --locked \
    --target x86_64-pc-windows-msvc --features gui --release --bin PortCanvasNative

#!/usr/bin/env bash
# Reproducible input/toolchain contract; the DMG filesystem itself is not byte-reproducible.
set -euo pipefail
app_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'macOS packaging requires macOS and the Xcode Command Line Tools.' >&2
  exit 1
fi
if [[ $# -ne 2 || "$1" != --output ]]; then
  echo 'Usage: bash app/tools/build-macos.sh --output /absolute/new-artifact-directory' >&2
  exit 2
fi
output="$2"
# Validate before spending time compiling, and refuse to replace existing output.
python3 -B - "$app_root" "$output" <<'PY'
import sys
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]) / 'tools'))
from macos_package import new_output_path
new_output_path(Path(sys.argv[2]))
PY
sdk_version="$(xcrun --sdk macosx --show-sdk-version)"
if (( ${sdk_version%%.*} < 26 )); then
  echo 'The native glass bridge requires the macOS 26 SDK (Xcode 26+ or matching Command Line Tools); the app still targets macOS 13.0.' >&2
  exit 1
fi
export MACOSX_DEPLOYMENT_TARGET=13.0
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$app_root/native-probe/target}"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.90.0}"
export PYTHONDONTWRITEBYTECODE=1
# Fetch development metadata too: offline attribution reads the complete locked graph.
cargo fetch --manifest-path "$app_root/native-probe/Cargo.toml" --locked \
  --target aarch64-apple-darwin --target x86_64-apple-darwin
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  # Rust 1.90's strip=symbols can create host proc-macro dylibs rejected by
  # newer macOS dyld (mis-aligned LINKEDIT string pool). Keep build tools intact;
  # the target application still uses the existing stripped release profile.
  cargo --config 'profile.release.build-override.strip="none"' build --manifest-path "$app_root/native-probe/Cargo.toml" --locked \
    --features gui --release --bin PortCanvasNative --target "$target"
done
mkdir -p -- "$CARGO_TARGET_DIR"
scratch="$(mktemp -d "$CARGO_TARGET_DIR/.portcanvas-notices.XXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT
python3 -B "$app_root/tools/native_notices.py" \
  --target aarch64-apple-darwin --target x86_64-apple-darwin --require-license-texts \
  --output "$scratch/THIRD-PARTY-NOTICES.txt" --inventory "$scratch/DEPENDENCIES.json"
version="$(cargo metadata --locked --no-deps --format-version 1 --manifest-path "$app_root/native-probe/Cargo.toml" | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "portcanvas-native-probe"))')"
revision="$(git -C "$app_root" rev-parse HEAD)"
source_state=clean
if [[ -n "$(git -C "$app_root" status --porcelain)" ]]; then source_state=modified; fi
epoch="${SOURCE_DATE_EPOCH:-$(git -C "$app_root" show -s --format=%ct HEAD)}"
python3 -B "$app_root/tools/macos_package.py" \
  --arm64 "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/PortCanvasNative" \
  --x86-64 "$CARGO_TARGET_DIR/x86_64-apple-darwin/release/PortCanvasNative" \
  --notices "$scratch/THIRD-PARTY-NOTICES.txt" --inventory "$scratch/DEPENDENCIES.json" \
  --version "$version" --source-revision "$revision" --source-state "$source_state" --source-date-epoch "$epoch" --output "$output"

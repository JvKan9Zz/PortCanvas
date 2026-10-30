"""Inventory a committed PortCanvas baseline without reading user data.

This is a source index for native migration, not a feature-equivalence check.
Run from any directory: python app/tools/native_baseline.py --ref v0.5.95
"""
import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path


def git(root, *args):
    return subprocess.check_output(
        ["git", "-C", str(root), *args], encoding="utf-8"
    ).strip()


def inventory(root, ref):
    commit = git(root, "rev-parse", "--verify", f"{ref}^{{commit}}")
    paths = git(root, "ls-tree", "-r", "--name-only", commit).splitlines()

    def source(path):
        return git(root, "show", f"{commit}:{path}")

    def located(path, pattern):
        return [
            {"line": number, "text": line.strip()}
            for number, line in enumerate(source(path).splitlines(), 1)
            if re.search(pattern, line)
        ]

    app = "app/src-tauri/src/app.rs"
    bridge = "app/frontend/bridge.ts"
    dispatcher = source(app).split("match action.as_str()", 1)[1].split(
        "#[cfg(test)]", 1
    )[0]
    arms = re.findall(r'((?:"[a-z]+:[a-z-]+"\s*\|?\s*)+)=>', dispatcher)
    actions = sorted({name for arm in arms for name in re.findall(r'"([^"]+)"', arm)})
    bridge_actions = sorted(set(re.findall(r"call\('([^']+)'", source(bridge))))
    cargo = tomllib.loads(source("app/src-tauri/Cargo.toml"))
    service_root = "app/core" if "app/core/src/lib.rs" in paths else "app/src-tauri"
    config = json.loads(source("app/src-tauri/tauri.conf.json"))
    package = json.loads(source("app/package.json"))
    rust = [p for p in paths if p.startswith(("app/src-tauri/src/", service_root + "/src/")) and p.endswith(".rs")]
    return {
        "commit": commit,
        "tree": git(root, "rev-parse", f"{commit}^{{tree}}"),
        "scope": "Windows x64; macOS migration excluded",
        "limitations": [
            "Indexes committed source only; does not inspect installed or running software.",
            "Action extraction follows the existing Rust match and TypeScript call syntax.",
            "Source references require manual review; presence is not functional validation.",
        ],
        "versions": {"npm": package["version"], "rust": cargo["package"]["version"],
                     "bundle": config["version"]},
        "actions": actions,
        "bridgeActions": bridge_actions,
        "bridgeActionsWithoutDispatcher": sorted(set(bridge_actions) - set(actions)),
        "tauriCommands": located(app, r"^async fn terminal_subscribe|^async fn studio_call|invoke_handler"),
        "events": located(bridge, r"^const ready="),
        "entrypoints": [p for p in paths if p.startswith(("app/frontend/", "app/ui/"))],
        "remoteHelpers": [p for p in paths if p.startswith(service_root + "/resources/") and p.endswith((".py", ".sh", ".bash"))],
        "rustModules": {
            path: {
                "platformReferences": located(path, r"tauri::|rfd::|tauri_plugin|cfg\(windows\)|windows_sys::"),
                "embeddedHelpers": located(path, r"include_str!|include_bytes!"),
                "testCount": len(re.findall(r"#\[(?:tokio::)?test(?:\([^]]*\))?\]", source(path))),
            }
            for path in rust
        },
        "npmDependencies": package["dependencies"],
        "npmBuildDependencies": package["devDependencies"],
        "rustFeatures": cargo["features"],
        "rustDependencies": cargo["dependencies"],
        "rustBuildDependencies": cargo["build-dependencies"],
        "serviceCrate": tomllib.loads(source("app/core/Cargo.toml")) if service_root == "app/core" else None,
        "dataReferences": {
            path: located(path, r"\.json|backgrounds|runtime-cache|WCS_DATA_DIR|credential_context|PREFIX|app_data_dir|document_dir|portable.flag")
            for path in [app, *([service_root + "/src/services.rs"] if service_root == "app/core" else []), *[service_root + "/src/" + name + ".rs" for name in
                               ["preferences", "profiles", "credentials", "terminal_pool", "logs", "migration"]]]
        },
        "installation": {"identifier": config["identifier"], "bundle": config["bundle"],
                         "updater": config["plugins"]["updater"]},
        "tests": [p for p in paths if p.startswith(("app/tests/", "app/src-tauri/tests/", "app/core/tests/"))],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ref", default="HEAD", help="Committed baseline (default: HEAD)")
    parser.add_argument("--output", type=Path, help="Write JSON here instead of stdout")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    result = inventory(root, args.ref)
    text = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text, encoding="utf-8")
    else:
        sys.stdout.reconfigure(encoding="utf-8")
        print(text, end="")


if __name__ == "__main__":
    main()

"""One-click reproducible .pcplugin packager for custom PortCanvas plugins."""
import gzip
import json
import sys
import tarfile
from pathlib import Path


def pack_plugin(source_dir: Path, output_file: Path) -> None:
    manifest_path = source_dir / "manifest.json"
    if not manifest_path.is_file():
        raise FileNotFoundError(f"Missing manifest.json in {source_dir}")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema") != "portcanvas.plugin/1":
        raise ValueError("manifest.json must declare schema 'portcanvas.plugin/1'")
    runtime = manifest.get("runtime") or {}
    if runtime.get("kind") == "local-worker":
        entry = runtime.get("entry", "")
        if not entry or not (source_dir / entry).is_file():
            raise FileNotFoundError(
                f"manifest.json declares local-worker entry '{entry}', which is missing under {source_dir}"
            )
    output_file.parent.mkdir(parents=True, exist_ok=True)
    with output_file.open("wb") as out, gzip.GzipFile(
        filename="", mode="wb", fileobj=out, mtime=0
    ) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.USTAR_FORMAT) as tar:
            for item in sorted(source_dir.rglob("*")):
                if item.is_symlink() or not item.is_file():
                    continue
                rel = item.relative_to(source_dir)
                if "__pycache__" in rel.parts or item.suffix == ".pyc" or item.name == "pack.py":
                    continue
                info = tarfile.TarInfo(rel.as_posix())
                info.size = item.stat().st_size
                info.mode = 0o755 if rel.parts[0] == "bin" else 0o644
                with item.open("rb") as data:
                    tar.addfile(info, data)
    print(f"Packed {manifest['id']} v{manifest['version']} -> {output_file} ({output_file.stat().st_size} bytes)")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("Usage: python pack.py <plugin_source_dir> <output.pcplugin>", file=sys.stderr)
        sys.exit(2)
    pack_plugin(Path(sys.argv[1]), Path(sys.argv[2]))

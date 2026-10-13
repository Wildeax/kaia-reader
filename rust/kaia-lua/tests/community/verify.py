#!/usr/bin/env python3
"""Check vendored file hashes without network access."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parent
manifest = json.loads((root / "manifest.json").read_text())
count = 0
for plugin in manifest:
    folder = root / (plugin["name"] + ".koplugin")
    expected = {item["path"] for item in plugin["files"]} | {"PINNED.txt"}
    actual = {str(path.relative_to(folder)) for path in folder.rglob("*") if path.is_file()}
    assert actual == expected, f"Unexpected file list in {folder.name}: {actual ^ expected}"
    for item in plugin["files"]:
        content = (folder / item["path"]).read_bytes()
        assert hashlib.sha256(content).hexdigest() == item["sha256"], item["path"]
        count += 1
    pin = (folder / "PINNED.txt").read_text()
    assert plugin["sha"] in pin and plugin["repo"] in pin, folder.name
print(f"Verified {count} upstream files in {len(manifest)} pinned plugins.")

#!/usr/bin/env python3
"""Package all desktop applications (Ocel, Chrome, Files, Terminal, Desktop, Compositor) into kernel_fs.img."""

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import sys

def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

def main():
    root = Path(__file__).resolve().parents[1]
    image_path = root / "kernel/src/embedded-x86_64/kernel_fs.img"
    rel_dir = root / "target/x86_64-unknown-none/release"

    apps_map = {
        "/bin/compositor": rel_dir / "service-compositor",
        "/bin/desktop": rel_dir / "desktop",
        "/bin/files": rel_dir / "files",
        "/bin/terminal": rel_dir / "terminal",
        "/bin/chrome": rel_dir / "chrome",
        "/bin/ocel": rel_dir / "ocel",
        "/bin/net": rel_dir / "service-net",
        "/bin/vfs": rel_dir / "service-vfs",
        "/bin/shell": rel_dir / "app-shell",
    }

    # Verify all artifacts exist
    missing = [path for dest, path in apps_map.items() if not path.exists()]
    if missing:
        sys.exit(f"Missing required release binaries: {missing}")

    # Sign the binaries
    print(f"Signing desktop binaries...")
    subprocess.check_call([
        "python3", str(root / "scripts/cellos-sign"), "--sign",
        *[str(p) for p in apps_map.values()]
    ])

    reader = load_module("cellos_profile_reader", root / "scripts/generate-boot-config.py")
    writer = load_module("cellos_profile_writer", root / "tools/mkfat32.py")

    files = reader.image_files(image_path)
    print(f"Loaded {len(files)} files from {image_path}")

    # Inject new desktop binaries
    for dest, src in apps_map.items():
        data = src.read_bytes()
        files[dest] = data
        print(f"  Injected {dest} ({len(data)} bytes from {src.name})")

    # Update /etc/cellos/autoload.toml to autoload desktop and shell
    autoload_toml = (
        b"# Applications start after the configured required services are ready.\n"
        b"version = 1\n\n"
        b"[[cells]]\n"
        b"name = \"desktop\"\n"
        b"path = \"/bin/desktop\"\n"
        b"after = [\"compositor\", \"input\"]\n"
        b"restart = \"always\"\n\n"
        b"[[cells]]\n"
        b"name = \"shell\"\n"
        b"path = \"/bin/shell\"\n"
        b"after = [\"compositor\", \"input\"]\n"
        b"restart = \"always\"\n"
    )
    files["/etc/cellos/autoload.toml"] = autoload_toml
    print("  Updated /etc/cellos/autoload.toml with desktop and shell entries")

    with tempfile.TemporaryDirectory(prefix="desktop-x86-", dir=image_path.parent) as staging:
        staging = Path(staging)
        pairs = []
        for index, (destination, data) in enumerate(sorted(files.items())):
            source = staging / str(index)
            source.write_bytes(data)
            pairs.append((str(source), destination))
        output = staging / "kernel_fs.img"
        writer.create_fat32_image(str(output), pairs)

        # Validate that all files were recorded
        out_files = reader.image_files(output)
        for dest in apps_map.keys():
            if dest not in out_files:
                raise ValueError(f"FAT image packaging missed required target {dest}")
        if "/etc/cellos/autoload.toml" not in out_files:
            raise ValueError("FAT image packaging missed /etc/cellos/autoload.toml")

        os.replace(output, image_path)

    print(f"Successfully packaged unified desktop image at {image_path} ({image_path.stat().st_size} bytes)")

if __name__ == "__main__":
    main()

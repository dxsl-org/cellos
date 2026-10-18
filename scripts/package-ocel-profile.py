#!/usr/bin/env python3
"""Replace only /bin/ocel in a prepared embedded FAT image, preserving its files."""
import argparse
import importlib.util
import os
from pathlib import Path
import tempfile


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("artifact", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    reader = load_module("cellos_profile_reader", root / "scripts/generate-boot-config.py")
    writer = load_module("cellos_profile_writer", root / "tools/mkfat32.py")
    files = reader.image_files(args.image)
    target = next((path for path in files if path.lower() == "/bin/ocel"), None)
    if target is None:
        parser.error("prepared image must already contain /bin/ocel")
    files[target] = args.artifact.read_bytes()
    with tempfile.TemporaryDirectory(prefix="ocel-profile-", dir=args.image.parent) as staging:
        staging = Path(staging)
        pairs = []
        for index, (destination, data) in enumerate(sorted(files.items())):
            source = staging / str(index)
            source.write_bytes(data)
            pairs.append((str(source), destination))
        output = staging / "kernel_fs.img"
        writer.create_fat32_image(str(output), pairs)
        # Never publish a partially rebuilt image or silently drop another cell.
        if reader.image_files(output) != files:
            raise ValueError("profile packaging changed unrelated image contents")
        os.replace(output, args.image)
    print("Packaged signed Ocel profile; preserved all other embedded files")


if __name__ == "__main__":
    main()

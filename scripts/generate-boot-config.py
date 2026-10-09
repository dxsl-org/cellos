#!/usr/bin/env python3
"""Generate image-specific boot config without changing editable source files.

Python 3.8+; Python 3.11+ additionally validates TOML with stdlib tomllib.
CELLOS_CONFIG_DIR (or --config-dir) selects all three operator files verbatim.
Without an override, config/ templates are selected by packaging features and
actual image artifacts; each cell's inline name/path/after fields guide selection.
"""

import argparse
import ast
import importlib.util
import json
import os
import mmap
from pathlib import Path
import re
import struct
import tempfile

try:
    import tomllib
except ImportError:
    tomllib = None

CONFIG_PATH = "/etc/cellos"
DEFAULT_TEMPLATES = Path(__file__).resolve().parent.parent / "config"
MAX_CONFIG_BYTES = 16384


def feature_set(value):
    result = set()
    for item in value.replace(" ", ",").split(","):
        if item.startswith("app-init/"):
            result.add(item.split("/", 1)[1])
        elif item and "/" not in item and item != "--features":
            result.add(item)
    return result


def selected(path, features):
    board = "board-rpi3" in features
    if path in ("/bin/config", "/bin/ai"):
        return "ai" in features
    if path == "/bin/input":
        return "input" in features
    if path in ("/bin/compositor", "/bin/fb-console"):
        return "ui" in features
    if path in ("/bin/kms", "/bin/desktop", "/bin/virtio-gpu"):
        return "ui" in features and not board
    if path == "/bin/bcm-display":
        return "ui" in features and board
    if path == "/bin/dwc2-usb":
        return "usb-host" in features
    if path == "/bin/virtio-net":
        return not board
    if path == "/bin/supervisor":
        return bool(features & {"supervisor", "hostile-backend-recovery"})
    if path == "/bin/net-broker":
        return "c2c-broker" in features
    if path == "/bin/silo":
        return "development-silo-provider" in features
    if path == "/bin/hypervisor":
        return bool(features & {"tier3-autostart", "hv-autostart"})
    return True


def validate_syntax(raw, source):
    if len(raw) > MAX_CONFIG_BYTES:
        raise ValueError("{}: exceeds {} bytes".format(source, MAX_CONFIG_BYTES))
    text = raw.decode("utf-8")
    if tomllib is not None:
        document = tomllib.loads(text)
        if document.get("version") != 1:
            raise ValueError("{}: version must be 1".format(source))
    return text


def inline_field(block, field, default=None):
    # This is template selection, not a replacement TOML parser. The kernel's
    # real TOML parser remains authoritative; only the repository templates need
    # inline selection fields on hosts without tomllib. Operator overrides are
    # copied byte-for-byte and have no such formatting constraint.
    match = re.search(r"^" + field + r"\s*=\s*(.+?)\s*$", block, re.MULTILINE)
    if match is None:
        return default
    value = match.group(1)
    if value in ("true", "false"):
        return value == "true"
    if tomllib is not None:
        return tomllib.loads(field + " = " + value)[field]
    try:
        return ast.literal_eval(value)
    except (SyntaxError, ValueError) as error:
        raise ValueError("template {} must be an inline value on Python <3.11".format(field)) from error


def generate_configs(files, features="", template_dir=None):
    """Return canonical path -> bytes, never modifying the source directory.

    files: (host source, canonical destination) pairs, including cell-store files.
    Explicit operator directories bypass profile filtering entirely.
    """
    override = template_dir or os.environ.get("CELLOS_CONFIG_DIR")
    directory = Path(override or DEFAULT_TEMPLATES)
    features = feature_set(features)
    artifacts = {"/" + dst.lstrip("/") for src, dst in files if Path(src).is_file()}
    result = {}
    selected_blocks = {}
    headers = {}
    for name in ("system", "services", "autoload"):
        source = directory / (name + ".toml")
        raw = source.read_bytes()
        text = validate_syntax(raw, source)
        if override or name == "system":
            result[CONFIG_PATH + "/" + name + ".toml"] = raw
            continue
        blocks = re.split(r"(?m)^\[\[cells\]\]\s*$", text)
        headers[name] = blocks[0].rstrip() + "\n"
        cells = []
        for block in blocks[1:]:
            path = inline_field(block, "path")
            cell_name = inline_field(block, "name")
            if not isinstance(path, str) or not isinstance(cell_name, str):
                raise ValueError("{}: cells need inline string name/path".format(source))
            enabled = inline_field(block, "enabled", True)
            if enabled and not selected(path, features):
                continue
            if enabled and path not in artifacts:
                if inline_field(block, "required", False) and path != "/bin/net":
                    raise ValueError("{}: required artifact missing: {}".format(source, path))
                continue
            cells.append({"path": path, "name": cell_name, "enabled": enabled,
                          "after": inline_field(block, "after", []), "block": block.strip()})
        selected_blocks[name] = cells
    if override:
        return result
    all_cells = selected_blocks["services"] + selected_blocks["autoload"]
    by_path = {cell["path"]: cell for cell in all_cells if cell["enabled"]}
    silo = by_path.get("/bin/silo")
    kms = by_path.get("/bin/kms")
    if silo and kms and silo["name"] not in kms["after"]:
        kms["after"].append(silo["name"])
        line = "after = " + json.dumps(kms["after"], ensure_ascii=False)
        if re.search(r"(?m)^after\s*=", kms["block"]):
            kms["block"] = re.sub(r"(?m)^after\s*=.*$", lambda _: line, kms["block"])
        else:
            kms["block"] += "\n" + line
    names = {cell["name"] for cell in all_cells if cell["enabled"]}
    for cell in all_cells:
        if cell["enabled"]:
            for dependency in cell["after"]:
                if dependency not in names:
                    raise ValueError("{}: unavailable dependency {}".format(cell["name"], dependency))
    if len(all_cells) > 32:
        raise ValueError("combined services/autoload exceeds 32 cells")
    for name, cells in selected_blocks.items():
        text = headers[name] + "".join("\n[[cells]]\n" + cell["block"] + "\n" for cell in cells)
        raw = text.encode("utf-8")
        validate_syntax(raw, "generated " + name + ".toml")
        result[CONFIG_PATH + "/" + name + ".toml"] = raw
    return result


def image_files(image, base_lba=0, wanted=None):
    """Read FAT16/FAT32 LFN files without allocating a copy of a large disk."""
    with open(image, "rb") as source:
        if not Path(image).is_file():
            # Block devices cannot be mapped using a stat-reported file length.
            # Read only the declared FAT partition, not the whole device.
            source.seek(base_lba * 512)
            header = source.read(512)
            sectors = struct.unpack_from("<I", header, 32)[0] or struct.unpack_from("<H", header, 19)[0]
            source.seek(base_lba * 512)
            return volume_files(source.read(sectors * 512), image, 0, wanted)
        with mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ) as data:
            return volume_files(data, image, base_lba, wanted)


def volume_files(data, image, base_lba, wanted):
    base = base_lba * 512
    bps = struct.unpack_from("<H", data, base + 11)[0]
    spc = data[base + 13]
    reserved = struct.unpack_from("<H", data, base + 14)[0]
    fats = data[base + 16]
    roots = struct.unpack_from("<H", data, base + 17)[0]
    fat_sectors = struct.unpack_from("<H", data, base + 22)[0]
    fat32 = roots == 0
    if fat32:
        fat_sectors = struct.unpack_from("<I", data, base + 36)[0]
    if bps != 512 or not spc or not fat_sectors or data[base + 510:base + 512] != b"\x55\xaa":
        raise ValueError("{}: expected a FAT boot volume at LBA {}".format(image, base_lba))
    fat_start = base + reserved * bps
    root_start = base + (reserved + fats * fat_sectors) * bps
    data_start = root_start + ((roots * 32 + bps - 1) // bps) * bps
    cluster_bytes = bps * spc
    entry_bytes = 4 if fat32 else 2
    end_of_chain = 0x0FFFFFF8 if fat32 else 0xFFF8

    def chain(cluster):
        contents = bytearray()
        seen = set()
        while 2 <= cluster < end_of_chain:
            if cluster in seen or cluster * entry_bytes >= fat_sectors * bps:
                raise ValueError("invalid FAT cluster chain")
            seen.add(cluster)
            start = data_start + (cluster - 2) * cluster_bytes
            block = data[start:start + cluster_bytes]
            if len(block) != cluster_bytes:
                raise ValueError("truncated FAT16 cluster")
            contents.extend(block)
            cluster = struct.unpack_from("<I" if fat32 else "<H", data,
                                         fat_start + cluster * entry_bytes)[0]
            if fat32:
                cluster &= 0x0FFFFFFF
        return bytes(contents)

    files = {}
    visited = set()

    def walk(directory, prefix):
        long_parts = {}
        for offset in range(0, len(directory), 32):
            entry = directory[offset:offset + 32]
            if len(entry) < 32 or entry[0] == 0:
                break
            if entry[0] == 0xE5:
                long_parts.clear()
                continue
            if entry[11] == 0x0F:
                if entry[0] & 0x40:
                    long_parts.clear()
                long_parts[entry[0] & 0x1F] = entry[1:11] + entry[14:26] + entry[28:32]
                continue
            if long_parts:
                units = b"".join(long_parts[key] for key in sorted(long_parts))
                name = units.decode("utf-16le").split("\0", 1)[0].rstrip("\uffff")
            else:
                base = entry[:8].decode("ascii").rstrip()
                extension = entry[8:11].decode("ascii").rstrip()
                name = base + ("." + extension if extension else "")
            long_parts.clear()
            if entry[11] & 0x08 or name in (".", ".."):
                continue
            if "/" in name or "\\" in name:
                raise ValueError("invalid FAT16 image entry")
            path = prefix + "/" + name
            if wanted is not None:
                path = path.lower()
                if path not in wanted and not any(item.startswith(path + "/") for item in wanted):
                    continue
            cluster = struct.unpack_from("<H", entry, 26)[0]
            if fat32:
                cluster |= struct.unpack_from("<H", entry, 20)[0] << 16
            if entry[11] & 0x10:
                if cluster in visited:
                    raise ValueError("cyclic FAT16 directories")
                visited.add(cluster)
                walk(chain(cluster), path)
            else:
                size = struct.unpack_from("<I", entry, 28)[0]
                contents = chain(cluster)
                if len(contents) < size:
                    raise ValueError("truncated FAT16 file: " + path)
                files[path] = contents[:size]

    if fat32:
        root_cluster = struct.unpack_from("<I", data, base + 44)[0]
        walk(chain(root_cluster), "")
    else:
        walk(data[root_start:root_start + roots * 32], "")
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--config-dir", type=Path)
    parser.add_argument("--features", default=os.environ.get("CELLOS_INIT_FEATURES", ""))
    parser.add_argument("--artifact", action="append", default=[], metavar="/bin/NAME=HOST_FILE")
    images = parser.add_mutually_exclusive_group()
    images.add_argument("--refresh-image", type=Path,
                        help="refresh config inside an existing ramdisk before building its kernel")
    images.add_argument("--extract-image", type=Path,
                        help="export the exact embedded config for a native board's persistent FAT partition")
    images.add_argument("--preserve-persistent", type=Path,
                        help="copy existing P1 services/autoload verbatim over staged disk defaults")
    parser.add_argument("--base-lba", type=int, default=2048)
    args = parser.parse_args()
    files = []
    for artifact in args.artifact:
        dst, separator, src = artifact.partition("=")
        if not separator or not dst.startswith("/bin/"):
            parser.error("--artifact must be /bin/NAME=HOST_FILE")
        files.append((src, dst))
    templates = args.config_dir or Path(os.environ.get("CELLOS_CONFIG_DIR") or DEFAULT_TEMPLATES)
    if args.output_dir.resolve() == templates.resolve():
        parser.error("output directory must not overwrite editable source templates")
    if args.preserve_persistent:
        if args.preserve_persistent.exists():
            with open(args.preserve_persistent, "rb") as previous:
                previous.seek(args.base_lba * 512)
                header = previous.read(512)
            # A fresh/legacy raw image may not have a FAT P1 at all. There are
            # then no operator config paths to preserve; do not require one.
            if (len(header) != 512 or header[510:512] != b"\x55\xaa" or
                    struct.unpack_from("<H", header, 11)[0] != 512 or not header[13]):
                return
            wanted = {CONFIG_PATH + "/" + name + ".toml" for name in ("services", "autoload")}
            preserved = image_files(args.preserve_persistent, args.base_lba, wanted)
            args.output_dir.mkdir(parents=True, exist_ok=True)
            for path, raw in preserved.items():
                (args.output_dir / Path(path).name).write_bytes(raw)
        return
    image = args.refresh_image or args.extract_image
    wanted = {CONFIG_PATH + "/" + name + ".toml" for name in ("system", "services", "autoload")}
    contents = image_files(image, wanted=wanted if args.extract_image else None) if image else {}
    if args.extract_image:
        configs = {}
        for name in ("system", "services", "autoload"):
            path = CONFIG_PATH + "/" + name + ".toml"
            if path not in contents:
                parser.error("ramdisk missing {}; rebuild it before building the kernel".format(path))
            configs[path] = contents[path]
    elif args.refresh_image:
        # Repack all existing files unchanged except the three config sources.
        # Temporary host files keep the shared writer as the sole image encoder.
        with tempfile.TemporaryDirectory(prefix="cellos-config-") as temporary:
            pairs = list(files)
            for index, (path, raw) in enumerate(contents.items()):
                host = Path(temporary) / str(index)
                host.write_bytes(raw)
                pairs.append((str(host), path))
            configs = generate_configs(pairs, args.features, args.config_dir)
            pairs = [(src, dst) for src, dst in pairs if dst not in configs]
            for path, raw in configs.items():
                host = Path(temporary) / Path(path).name
                host.write_bytes(raw)
                pairs.append((str(host), path))
            writer_path = DEFAULT_TEMPLATES.parent / "tools" / "mkfat32.py"
            spec = importlib.util.spec_from_file_location("cellos_fat_writer", writer_path)
            writer = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(writer)
            staged = str(args.refresh_image) + ".new"
            writer.create_fat32_image(staged, pairs)
            Path(staged).replace(args.refresh_image)
    else:
        configs = generate_configs(files, args.features, args.config_dir)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for path, raw in configs.items():
        (args.output_dir / Path(path).name).write_bytes(raw)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Install an exact reviewed Lambda Python wheel set without pip or network."""

import re
import sys
from email.parser import Parser
from pathlib import Path, PurePosixPath
from zipfile import ZipFile

_REQUIREMENT = re.compile(r"([A-Za-z0-9_.]+)==([A-Za-z0-9_.]+)").fullmatch
_WHEEL = re.compile(
    r"([A-Za-z0-9_.]+)-([A-Za-z0-9_.]+)(?:-[A-Za-z0-9_.]+)?-"
    r"([A-Za-z0-9_.]+)-([A-Za-z0-9_.]+)-([A-Za-z0-9_.]+)\.whl"
).fullmatch
_ALLOWED_PLATFORMS = {
    "any",
    "linux_x86_64",
    "manylinux2014_x86_64",
    "manylinux_2_17_x86_64",
    "manylinux_2_28_x86_64",
}
_MAX_FILES = 5000
_MAX_FILE_BYTES = 32 * 1024 * 1024
_MAX_TOTAL_BYTES = 96 * 1024 * 1024


def _fail() -> None:
    raise SystemExit("offline wheel installation failed")


def _normalized(value: str) -> str:
    return value.lower().replace("_", "-").replace(".", "-")


def _compatible_tag(python: str, abi: str, platform: str) -> bool:
    if platform == "any":
        return python == "py3" and abi == "none"
    if platform not in _ALLOWED_PLATFORMS:
        return False
    if python == "cp37":
        return abi == "abi3"
    if python == "cp312":
        return abi in {"cp312", "abi3", "none"}
    return python == "py3" and abi == "none"


def _compatible(python: str, abi: str, platform: str) -> bool:
    return any(
        _compatible_tag(python_tag, abi_tag, platform_tag)
        for python_tag in python.split(".")
        for abi_tag in abi.split(".")
        for platform_tag in platform.split(".")
    )


def _expanded_tags(python: str, abi: str, platform: str) -> set[str]:
    return {
        f"{python_tag}-{abi_tag}-{platform_tag}"
        for python_tag in python.split(".")
        for abi_tag in abi.split(".")
        for platform_tag in platform.split(".")
    }


def _requirements(path: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    try:
        for line in path.read_text(encoding="ascii").splitlines():
            match = _REQUIREMENT(line)
            if match is None:
                _fail()
            name = _normalized(match.group(1))
            if name in result:
                _fail()
            result[name] = match.group(2)
    except (OSError, UnicodeError):
        _fail()
    if not result:
        _fail()
    return result


def _selected_wheels(
    wheelhouse: Path, requirements: dict[str, str]
) -> list[tuple[Path, str, str, set[str]]]:
    selected: dict[str, tuple[Path, str, str, set[str]]] = {}
    for path in wheelhouse.glob("*.whl"):
        if not path.is_file() or path.is_symlink():
            _fail()
        match = _WHEEL(path.name)
        if match is None:
            _fail()
        name, version, python, abi, platform = match.groups()
        normalized = _normalized(name)
        if requirements.get(normalized) != version or not _compatible(python, abi, platform):
            continue
        if normalized in selected:
            _fail()
        selected[normalized] = (
            path, normalized, version, _expanded_tags(python, abi, platform),
        )
    if set(selected) != set(requirements):
        _fail()
    return [selected[name] for name in sorted(selected)]


def _target_name(name: str) -> PurePosixPath:
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts or not path.parts:
        _fail()
    if ".data" in path.parts[0]:
        prefix, category, *remaining = path.parts
        if (
            not prefix.endswith(".data")
            or category not in {"purelib", "platlib"}
            or not remaining
        ):
            _fail()
        path = PurePosixPath(*remaining)
    return path


def _require_archive_contract(
    archive: ZipFile, name: str, version: str, filename_tags: set[str]
) -> list:
    infos = [info for info in archive.infolist() if not info.is_dir()]
    if len(infos) > _MAX_FILES or any(info.file_size > _MAX_FILE_BYTES for info in infos):
        _fail()
    metadata = [info for info in infos if info.filename.endswith(".dist-info/METADATA")]
    wheel = [info for info in infos if info.filename.endswith(".dist-info/WHEEL")]
    if len(metadata) != 1 or len(wheel) != 1:
        _fail()
    if PurePosixPath(metadata[0].filename).parent != PurePosixPath(wheel[0].filename).parent:
        _fail()
    try:
        headers = Parser().parsestr(archive.read(metadata[0]).decode("utf-8"))
        wheel_text = archive.read(wheel[0]).decode("utf-8")
    except (KeyError, UnicodeError):
        _fail()
    declared_tags = {
        line[5:].strip() for line in wheel_text.splitlines() if line.startswith("Tag: ")
    }
    compatible_tags = {
        tag
        for tag in declared_tags
        if len(parts := tag.split("-")) == 3 and _compatible(*parts)
    }
    if (
        _normalized(headers.get("Name", "")) != name
        or headers.get("Version") != version
        or declared_tags != filename_tags
        or not compatible_tags
    ):
        _fail()
    return infos


def install_wheels(requirements_path: Path, wheelhouse: Path, target: Path) -> None:
    """Extract one compatible exact wheel per pin into an empty target directory."""
    if not wheelhouse.is_dir() or not target.is_dir() or any(target.iterdir()):
        _fail()
    wheels = _selected_wheels(wheelhouse, _requirements(requirements_path))
    written: set[PurePosixPath] = set()
    total = 0
    count = 0
    for wheel, name, version, tags in wheels:
        with ZipFile(wheel) as archive:
            for info in _require_archive_contract(archive, name, version, tags):
                count += 1
                total += info.file_size
                if count > _MAX_FILES or total > _MAX_TOTAL_BYTES:
                    _fail()
                relative = _target_name(info.filename)
                if (
                    relative in written
                    or (info.external_attr >> 16) & 0o170000 == 0o120000
                ):
                    _fail()
                written.add(relative)
                destination = target.joinpath(*relative.parts)
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(archive.read(info))


def main(arguments: list[str]) -> None:
    """Accept exactly ``REQUIREMENTS WHEELHOUSE TARGET`` or fail closed."""
    if len(arguments) != 3:
        _fail()
    install_wheels(Path(arguments[0]), Path(arguments[1]), Path(arguments[2]))


if __name__ == "__main__":
    main(sys.argv[1:])

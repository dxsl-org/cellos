#!/bin/sh
set -eu

usage() {
    echo "usage: package.sh --manifest FILE --wheelhouse DIR --output FILE" >&2
    exit 2
}

manifest=
wheelhouse=
output=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --manifest) [ "$#" -ge 2 ] || usage; manifest=$2; shift 2 ;;
        --wheelhouse) [ "$#" -ge 2 ] || usage; wheelhouse=$2; shift 2 ;;
        --output) [ "$#" -ge 2 ] || usage; output=$2; shift 2 ;;
        *) usage ;;
    esac
done
[ -n "$manifest" ] && [ -f "$manifest" ] || usage
[ -n "$wheelhouse" ] && [ -d "$wheelhouse" ] || usage
[ -n "$output" ] && [ ! -e "$output" ] && [ -d "$(dirname -- "$output")" ] || usage

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
staging=$(mktemp -d)
trap 'rm -rf -- "$staging"' EXIT HUP INT TERM

python3 "$root/scripts/verify_wheelhouse.py" "$wheelhouse"
PYTHONPATH="$root/src" python3 -c \
    'import sys; from pathlib import Path; from manifest import decode_manifest; decode_manifest(Path(sys.argv[1]).read_bytes())' \
    "$manifest"
python3 "$root/scripts/install_wheels.py" \
    "$root/requirements.txt" "$wheelhouse" "$staging"
cp "$root"/src/*.py "$staging"/
cp "$manifest" "$staging/manifest.json"
python3 "$root/scripts/package_zip.py" "$staging" "$output"

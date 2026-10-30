#!/usr/bin/env bash
# One-time, authorized fetch of the OpenSSH apk set for the Tier-3 guest endpoint
# (S10-11 T3-A, CP1 authorization A-NET). It is NEVER run by a lane.
#
# One HTTPS session (a single http.client connection, GET only) to dl-cdn.alpinelinux.org:
#   1. GET /alpine/v3.21/main/x86_64/APKINDEX.tar.gz (skipped when $W/apk-fetch already holds
#      exactly that file from an earlier aborted session: it is then reused, see fetch-record.json)
#   2. resolve the dependency closure of openssh-server, openssh-server-common and
#      openssh-keygen from that index (the guest installs offline with an empty apk
#      database, so every `so:`/`cmd:` dependency must come from the set)
#   3. GET each resolved .apk
# Every file lands in $W/apk-fetch; the sha256 of each is recorded in
# tests/guests/x86-sshd/apk-pins.sha256 (sha256sum -c format, relative names) and in
# $W/apk-fetch/fetch-record.json. No other host, no credentials, no redirect is followed,
# the script refuses to run when $W/apk-fetch holds anything but a lone APKINDEX.tar.gz.
#
# Usage: CELLBOX_TIER3_WORK=/tmp/cellbox-tier3-XXXX bash scripts/fetch-tier3-sshd-apks-x86.sh
set -euo pipefail

ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
W="${CELLBOX_TIER3_WORK:?set CELLBOX_TIER3_WORK to the owned /tmp/cellbox-tier3-* directory}"
case "$W" in /tmp/cellbox-tier3-*) ;; *) echo "FAIL: work directory must be /tmp/cellbox-tier3-*" >&2; exit 1 ;; esac
[[ -d "$W" && ! -L "$W" ]] || { echo "FAIL: work directory missing or a symlink: $W" >&2; exit 1; }
[[ "$(stat -c %u "$W")" == "$(id -u)" ]] || { echo "FAIL: work directory not owned by this uid" >&2; exit 1; }
FETCH="$W/apk-fetch"
if [[ -e "$FETCH" ]] && [[ -n "$(ls -A "$FETCH")" ]] && [[ "$(ls -A "$FETCH")" != "APKINDEX.tar.gz" ]]; then
    echo "FAIL: $FETCH is not empty: the authorized fetch is single-use" >&2
    exit 1
fi
PINS="$ROOT/tests/guests/x86-sshd/apk-pins.sha256"
mkdir -p "$FETCH" "$(dirname "$PINS")"
PYTHON_BIN="${PYTHON_BIN:-python3}"

"$PYTHON_BIN" - "$FETCH" "$PINS" <<'PY'
import gzip, hashlib, http.client, io, json, re, sys, tarfile
from pathlib import Path

HOST = "dl-cdn.alpinelinux.org"
BASE = "/alpine/v3.21/main/x86_64/"
ROOTS = ["openssh-server", "openssh-server-common", "openssh-keygen"]
# `/bin/sh` has three providers in the index; Alpine's default is busybox-binsh.
PREFER = {"/bin/sh": "busybox-binsh"}
fetch, pins = Path(sys.argv[1]), Path(sys.argv[2])


def parse_index(text):
    packages = []
    for block in text.split("\n\n"):
        fields = {}
        for line in block.splitlines():
            if len(line) > 2 and line[1] == ":":
                fields[line[0]] = line[2:]
        if "P" in fields:
            packages.append(fields)
    return packages


def dep_name(token):
    return re.split(r"[=<>~]", token, maxsplit=1)[0]


def resolve(packages, roots):
    by_name = {p["P"]: p for p in packages}
    providers = {}
    for p in packages:
        providers.setdefault(p["P"], []).append(p["P"])
        for token in p.get("p", "").split():
            providers.setdefault(dep_name(token), []).append(p["P"])
    chosen, order, todo = {}, [], list(roots)
    while todo:
        name = todo.pop(0)
        if name in chosen:
            continue
        if name not in by_name:
            raise SystemExit("FAIL: package %s is not in the index" % name)
        chosen[name] = by_name[name]
        order.append(name)
        for token in by_name[name].get("D", "").split():
            if token.startswith("!"):
                continue
            want = dep_name(token)
            candidates = sorted(set(providers.get(want, [])))
            if not candidates:
                raise SystemExit("FAIL: %s needs %s, which nothing in the index provides" % (name, token))
            if want in by_name:
                pick = want
            elif want in PREFER:
                pick = PREFER[want]
                if pick not in candidates:
                    raise SystemExit("FAIL: preferred provider %s of %s is not in the index" % (pick, want))
            elif len(candidates) == 1:
                pick = candidates[0]
            else:
                raise SystemExit("FAIL: %s needs %s: several providers %s" % (name, token, candidates))
            todo.append(pick)
    return [chosen[n] for n in order]


def main():
    conn = http.client.HTTPSConnection(HOST, 443, timeout=120)
    requests = []

    def get(path):
        conn.request("GET", path, headers={"User-Agent": "cellos-tier3-fetch/1", "Connection": "keep-alive"})
        r = conn.getresponse()
        body = r.read()
        requests.append({"host": HOST, "method": "GET", "path": path, "status": r.status, "bytes": len(body)})
        if r.status != 200:
            raise SystemExit("FAIL: GET %s -> %s (no redirect is followed)" % (path, r.status))
        return body

    kept = fetch / "APKINDEX.tar.gz"
    index_reused = kept.exists()
    if index_reused:
        index_bytes = kept.read_bytes()
    else:
        index_bytes = get(BASE + "APKINDEX.tar.gz")
        kept.write_bytes(index_bytes)
    stream = gzip.GzipFile(fileobj=io.BytesIO(index_bytes)).read()
    text = None
    with tarfile.open(fileobj=io.BytesIO(stream)) as tar:
        for member in tar:
            if member.name == "APKINDEX":
                text = tar.extractfile(member).read().decode()
    if text is None:
        raise SystemExit("FAIL: no APKINDEX member in APKINDEX.tar.gz")
    packages = parse_index(text)
    resolved = resolve(packages, ROOTS)
    record = {"host": HOST, "base": BASE, "roots": ROOTS, "index_packages": len(packages),
              "index_reused_from_earlier_session": index_reused, "packages": []}
    pin_lines = ["%s  APKINDEX.tar.gz" % hashlib.sha256(index_bytes).hexdigest()]
    for p in resolved:
        fname = "%s-%s.apk" % (p["P"], p["V"])
        if "/" in fname or ".." in fname:
            raise SystemExit("FAIL: unsafe file name %r" % fname)
        data = get(BASE + fname)
        (fetch / fname).write_bytes(data)
        digest = hashlib.sha256(data).hexdigest()
        pin_lines.append("%s  %s" % (digest, fname))
        record["packages"].append({"name": p["P"], "version": p["V"], "file": fname, "bytes": len(data),
                                   "sha256": digest, "index_size": p.get("S"), "depends": p.get("D", ""),
                                   "provides": p.get("p", "")})
    conn.close()
    record["requests"] = requests
    (fetch / "fetch-record.json").write_text(json.dumps(record, indent=2) + "\n")
    pins.write_text("\n".join(pin_lines) + "\n")
    print("fetched %d files in one session (%d GETs):" % (len(pin_lines), len(requests)))
    print("\n".join(pin_lines))


if __name__ == "__main__":
    main()
PY
echo "pins written: $PINS"

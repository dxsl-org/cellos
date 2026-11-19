#!/usr/bin/env bash
# Register self-hosted GitHub Actions runners for this repository on this
# machine, each as a systemd --user service, labelled `cellos-ci`.
# Workflows pick them when the repository variable CI_SELF_HOSTED is "true"
# (see the runner-selection comment at the top of .github/workflows/ci.yml).
#
#   scripts/ci-runner/setup-runner.sh install [COUNT]   # default COUNT=3
#   scripts/ci-runner/setup-runner.sh remove
#   scripts/ci-runner/setup-runner.sh status
#
# Isolation contract: runners run with their own HOME ($CI_HOME), so the
# workflow's rustup/cargo installs, `pip --user`, ~/.cache (QEMU 10.2 build,
# apt .debs) and the actions' caches never touch the developer's toolchains.
# The environment is rebuilt from scratch, so variables a login shell exports
# (CARGO_BUILD_TARGET, CC_*) do not leak into jobs. Jobs cannot sudo: apt steps
# only verify packages, so `install` prints the one apt command to run first.
#
# Requires: gh authenticated with admin rights on the repository (it mints the
# short-lived registration/removal tokens), systemd user services, curl, tar.
set -euo pipefail

REPO="${CELLOS_CI_REPO:-dxsl-org/cellos}"
BASE="${CELLOS_CI_BASE:-$HOME/.cellos-ci}"
CI_HOME="$BASE/home"
LABEL="cellos-ci"
TOOLCHAIN="nightly-2026-05-01"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
UNIT_DIR="$HOME/.config/systemd/user"
UNIT="cellos-ci-runner@.service"
CLEAN_PATH="$CI_HOME/.cargo/bin:$CI_HOME/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

runner_dirs() { compgen -G "$BASE/runner-*" || true; }

host_package_check() {
    local pkgs
    pkgs=$(python3 - "$ROOT" <<'EOF'
import pathlib, sys, yaml
root = pathlib.Path(sys.argv[1])
names = set()
for wf in (root / ".github/workflows").glob("*.yml"):
    for job in (yaml.safe_load(wf.read_text()).get("jobs") or {}).values():
        for step in job.get("steps", []):
            if step.get("uses") == "./.github/actions/apt-install":
                packages = step["with"]["packages"]
                if "${{" not in packages:  # matrix-driven: read below
                    names.update(packages.split())
for entry in (yaml.safe_load((root / ".github/workflows/ci.yml").read_text())
              ["jobs"]["build"]["strategy"]["matrix"]["include"]):
    names.update((entry.get("cc_pkg") or "").split())
print(" ".join(sorted(names)))
EOF
)
    if APT_PACKAGES="$pkgs" bash "$ROOT/.github/actions/apt-install/apt-install.sh" check; then
        return 0
    fi
    echo "Install the packages above (needs sudo), then rerun this script." >&2
    return 1
}

prepare_home() {
    mkdir -p "$CI_HOME/.cargo/bin" "$CI_HOME/.local/bin"
    if [[ ! -x "$CI_HOME/.cargo/bin/rustup" ]]; then
        local init="$BASE/rustup-init"
        cp "$(command -v rustup)" "$init"
        RUSTUP_HOME="$CI_HOME/.rustup" CARGO_HOME="$CI_HOME/.cargo" \
            "$init" -y --no-modify-path --profile minimal --default-toolchain none
        rm -f "$init"
    fi
    local env=(env -i HOME="$CI_HOME" PATH="$CLEAN_PATH")
    # Components and targets up front: concurrent jobs' rust-toolchain steps
    # then find them installed instead of racing on the same download.
    "${env[@]}" rustup toolchain install "$TOOLCHAIN" --profile minimal \
        --component rust-src,rustfmt,clippy \
        --target riscv64gc-unknown-none-elf,aarch64-unknown-none-softfloat,x86_64-unknown-none
    "${env[@]}" rustup default "$TOOLCHAIN"
    local tool
    for tool in cargo-deny cargo-audit; do
        [[ -x "$CI_HOME/.cargo/bin/$tool" ]] \
            || "${env[@]}" cargo +"$TOOLCHAIN" install --locked "$tool"
    done
}

fetch_runner() {
    local tag tarball
    tag=$(gh api repos/actions/runner/releases/latest --jq .tag_name)
    tarball="$BASE/dist/actions-runner-linux-x64-${tag#v}.tar.gz"
    if [[ ! -f "$tarball" ]]; then
        mkdir -p "$BASE/dist"
        curl -fsSL -o "$tarball.part" \
            "https://github.com/actions/runner/releases/download/$tag/$(basename "$tarball")"
        mv "$tarball.part" "$tarball"
    fi
    echo "$tarball"
}

write_unit() {
    mkdir -p "$UNIT_DIR"
    cat > "$UNIT_DIR/$UNIT" <<EOF
[Unit]
Description=CellOS GitHub Actions runner %i
After=network-online.target

[Service]
WorkingDirectory=$BASE/runner-%i
ExecStart=/usr/bin/env -i HOME=$CI_HOME PATH=$CLEAN_PATH LANG=C.UTF-8 $BASE/runner-%i/run.sh
Restart=always
RestartSec=10
KillMode=process

[Install]
WantedBy=default.target
EOF
    systemctl --user daemon-reload
}

install() {
    local count="${1:-3}" tarball token i dir
    host_package_check
    prepare_home
    tarball=$(fetch_runner)
    write_unit
    token=$(gh api -X POST "repos/$REPO/actions/runners/registration-token" --jq .token)
    for ((i = 1; i <= count; i++)); do
        dir="$BASE/runner-$i"
        if [[ ! -f "$dir/.runner" ]]; then
            mkdir -p "$dir"
            tar -xzf "$tarball" -C "$dir"
            (cd "$dir" && env -i HOME="$CI_HOME" PATH="$CLEAN_PATH" ./config.sh --unattended \
                --url "https://github.com/$REPO" --token "$token" \
                --name "$(hostname)-cellos-$i" --labels "$LABEL" --work _work --replace)
        fi
        # Job environment (the runner adds these to every step).
        printf 'HOME=%s\nPATH=%s\nLANG=C.UTF-8\nPIP_BREAK_SYSTEM_PACKAGES=1\n' \
            "$CI_HOME" "$CLEAN_PATH" > "$dir/.env"
        systemctl --user enable --now "cellos-ci-runner@$i.service"
    done
    loginctl enable-linger "$USER" 2>/dev/null \
        || echo "note: enable-linger failed; runners stop when your last session ends" >&2
    echo "Registered $count runner(s). Route jobs to them with:"
    echo "  gh variable set CI_SELF_HOSTED --repo $REPO --body true"
}

remove() {
    local token dir i
    token=$(gh api -X POST "repos/$REPO/actions/runners/remove-token" --jq .token)
    for dir in $(runner_dirs); do
        i="${dir##*-}"
        systemctl --user disable --now "cellos-ci-runner@$i.service" || true
        (cd "$dir" && env -i HOME="$CI_HOME" PATH="$CLEAN_PATH" ./config.sh remove --token "$token")
        rm -rf "$dir"
    done
    echo "Runners removed. Jobs need hosted runners again:"
    echo "  gh variable delete CI_SELF_HOSTED --repo $REPO"
}

status() {
    local dir
    for dir in $(runner_dirs); do
        systemctl --user --no-pager --lines=0 status "cellos-ci-runner@${dir##*-}.service" || true
    done
    gh api "repos/$REPO/actions/runners" --jq '.runners[] | "\(.name)\t\(.status)\tbusy=\(.busy)"'
}

case "${1:-}" in
install) install "${2:-3}" ;;
remove) remove ;;
status) status ;;
*) sed -n '2,10p' "$0" >&2; exit 2 ;;
esac

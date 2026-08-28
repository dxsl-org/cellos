#!/usr/bin/env bash

run_scoped_process_start() {
    local stat tail
    [[ "$1" =~ ^[0-9]+$ && -r "/proc/$1/stat" ]] || return 1
    IFS= read -r stat < "/proc/$1/stat" || return 1
    tail="${stat##*) }"
    set -- $tail
    [[ -n "${20:-}" ]] || return 1
    printf '%s\n' "${20}"
}

terminate_run_scoped_process_group() {
    local pid="${1:-}" expected_start="${2:-}" current_start
    current_start="$(run_scoped_process_start "$pid")" || return 0
    [[ "$current_start" == "$expected_start" ]] || return 0

    kill -TERM -- "-$pid" 2>/dev/null || true
    for _ in {1..50}; do
        current_start="$(run_scoped_process_start "$pid")" || return 0
        [[ "$current_start" == "$expected_start" ]] || return 0
        sleep 0.1
    done
    kill -KILL -- "-$pid" 2>/dev/null || true
}

cleanup_stale_run_scoped_workspaces() {
    local prefix="$1" stale owner_pid workload_pid workload_start
    shopt -s nullglob
    for stale in "$prefix".*; do
        [[ -f "$stale/owner.pid" ]] || continue
        IFS= read -r owner_pid < "$stale/owner.pid" || owner_pid=
        if [[ "$owner_pid" =~ ^[0-9]+$ ]] && kill -0 "$owner_pid" 2>/dev/null; then
            continue
        fi
        if [[ -f "$stale/workload.pid" ]]; then
            read -r workload_pid workload_start < "$stale/workload.pid" || true
            terminate_run_scoped_process_group "$workload_pid" "$workload_start"
        fi
        rm -rf -- "$stale"
    done
    shopt -u nullglob
}

#!/usr/bin/env bash
# Shared operator-policy helper for the image-assembly scripts.
#
#   bake_policy <out-path>              generate the signed blob
#   assert_policy_in_image <layout>     check a saved inspect_fat.py listing
#
# WHY every image wants the blob: without /POLICY.BIN the kernel takes the `Absent`
# branch, which is dev-permissive — the loader, verifier, parser and narrowing rule
# all run and change nothing. An image with no blob is not "policy off", it is
# "policy present in the code and inert on the device", and no test can tell.
#
# Requires `$PYTHON_BIN` (the caller's probed interpreter) to be set already.

# Generate the signed blob at $1. sign-policy.py round-trip-decodes its own output
# with an independent decoder before writing, so an entry outside the kernel's domain
# masks fails HERE. That matters more than it sounds: a blob the kernel rejects
# becomes PolicyState::Invalid → DenyAll for EVERY path, and a "boots to a prompt"
# check cannot catch it because the shell is trusted-core and comes up regardless.
#
# The blob is signed with the DEV fleet key and only verifies while the kernel carries
# the default `dev-policy-key` feature. An image containing it built without that
# feature is Invalid → DenyAll for every cell outside vfs/shell/net.
bake_policy() {
    local out="$1"
    "$PYTHON_BIN" scripts/sign-policy.py --out "$out" >/dev/null
    if [[ ! -s "$out" ]]; then
        echo "FAIL: sign-policy.py produced no blob (need 'pip install cryptography')" >&2
        return 1
    fi
}

# Assert the blob landed at the root as /POLICY.BIN, given an inspect_fat.py listing.
# The kernel reads exactly that path (8.3-uppercase, root level); anywhere else and it
# reports "absent" and silently falls back to dev-permissive — an image that looks
# provisioned and enforces nothing. mkfat32 exits 0 in that case, so the exit code
# proves nothing.
assert_policy_in_image() {
    local layout="$1"
    if ! grep -q -- 'SFN POLICY.BIN' "$layout"; then
        echo "FAIL: image has no /POLICY.BIN in the root directory" >&2
        cat "$layout" >&2
        return 1
    fi
}

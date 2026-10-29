#!/usr/bin/env bash
# S10-5 P2 evidence: the G3/cloud negative controls and the hashes of every S10-2b file.
# The controls that name the REAL quarantined kernel/image are only meaningful while those are
# still canonical (before P3); this script therefore takes the kernel/image paths as arguments
# and records what it ran. Read-only apart from the scratch directory.
# usage: p2_controls.sh <kernel> <embedded-dir> <scratch>
set -u
cd /home/dmin/cellos
K="$1"; EMB="$2"; S="$3"
echo "kernel sha256: $(sha256sum "$K" | cut -d' ' -f1)"
echo "image  sha256: $(sha256sum "$EMB/kernel_fs.img" | cut -d' ' -f1)"
python3 scripts/x86/check_default_kernel.py "$K" "$EMB"; echo "check_default_kernel.py exit=$?"
# (The default-path `make-iso-ci.sh` refusal on the real quarantined kernel was observed before P3:
#  exit 1 "quarantined bytes in the image: /bin/cellbox", no ISO written; see proposal S10-5.13.
#  After P3 the default kernel is the promoted one and is admitted, so it is not rerun here.)
bash scripts/build-cloud-images.sh; echo "build-cloud-images.sh exit=$?"
echo "--- S10-2b file hashes"
sha256sum scripts/assemble-x86-image.py scripts/cellos_sign/signing.py scripts/cellos_sign/cli.py \
  scripts/package-desktop-x86.py scripts/x86/make-iso-ci.sh scripts/x86/check_default_kernel.py \
  scripts/build-cloud-images.sh scripts/test_cellos_sign.py scripts/test_assemble_x86_image.py \
  scripts/test_check_default_kernel.py .github/workflows/ci.yml

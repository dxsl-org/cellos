# S2 phase 06 — file-set extension: `/bin/supervisor` in the x86 image

Recorded 2026-11-05. The owner (the user) answered **"Cho phép"**: the S2 file-bounded exception is extended so that x86 images package and start `/bin/supervisor`, which H-S2-6 (a) and H-S2-7 require. Blocker B-1 of phase 06.

**Added paths:** `scripts/assemble-x86-image.py` (supervisor row in the headless and desktop cell lists), `scripts/cellos_sign/signing.py` (assembler pin and signed-binary set), `scripts/test_assemble_x86_image.py`, and the boot-config row selection that starts the supervisor from `config/services.toml` (the existing supervisor template row). Anything else needed is reported, not edited.

**Effect:** every x86 image runs the supervisor service. Recorded as a CP2-06 deviation; CP2-06 remains the owner's decision. All 38 lock relations keep their whole-plan scope.

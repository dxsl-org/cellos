# Phase 03 — ViUI demo and evidence

## Context Links
Approved spec AC-1/AC-9/AC-10; `cells/demos/viui-demo`; `cells/apps/robot-dashboard/src/main.rs`.
## Overview
Turn the compile-only generated component demo into a live managed surface.
## Key Insights
Use existing input polling and cooperative yield loop; compositor owns decoration.
## Requirements
Set title, render generated `.vi` content, remain interactive, exit only on accepted close.
## Architecture
Create surface → framebuffer renderer/managed app → input+lifecycle loop.
## Related Code Files
`cells/demos/viui-demo/src/main.rs`; manifest only if syscall metadata requires it.
## Implementation Steps
Wire generated component into managed runner, add diagnostics, build and gather host/QEMU evidence.
## Todo List
- [x] Implement live demo loop.
- [x] Verify host/RISC-V tests, signed image, dedicated RV64 QEMU oracle, and separate window-policy regression.
## Success Criteria
Demo compiles as a real compositor client and all approved regression gates pass.
## Risk Assessment
Boot-time service races require bounded cooperative retry behavior.
## Security Considerations
Declare only syscalls reached by the real loop; no new protocol authority.
## Next Steps
Reopen only under separate approval for broader desktop behavior, physical-board
validation, or production qualification.

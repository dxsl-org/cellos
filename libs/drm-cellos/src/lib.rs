#![no_std]
//! drm-cellos — link-only crate for libdrm-lite (runtime plan R6).
//!
//! Its build script compiles the pinned libdrm subset
//! (`third_party/libdrm`), the Cellos device-enumeration backend and `ioctl`
//! into `libdrm_lite.a`, plus the test-only fake provider with the
//! `fake-provider` feature, for x86_64 Tier-2 Cells. A Cell links them with
//! `extern crate drm_cellos;`; its C code reaches the headers through
//! `DEP_DRM_LITE_INCLUDE`.
//!
//! The libdrm functions present are exactly those in
//! `third_party/libdrm/README.md`; any other libdrm symbol stays undefined,
//! so a new dependency fails at link time instead of misbehaving.
//!
//! The render node is served by an fd provider registered with
//! `cellos_fd_provider_register` (mlibc vicell sysdeps,
//! `third_party/mlibc/sysdeps/vicell/include/vicell/fd_provider.h`).

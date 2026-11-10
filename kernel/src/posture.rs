// SPDX-License-Identifier: MPL-2.0
//! Trust posture derivation, shared by `kernel/build.rs` (which enforces it and
//! emits `cfg(cellos_posture = …)`) and the kernel host tests.
//!
//! A kernel has exactly one posture:
//! - `Release`: carries the release cell anchor (`release-posture`,
//!   `production-relay-image`). It may not also carry a development, test,
//!   recovery or private-fixture feature, and keeps fail-closed policy and
//!   signing.
//! - `Test`: `test-hooks`.
//! - `Dev`: everything else.
//!
//! On 64-bit targets every bootable kernel needs `native-domains`, because
//! without it the user-copy boundary is a raw memcpy; only `compile-only`
//! builds (which never boot) are exempt.

/// Features a release kernel may never carry.
pub const FORBIDDEN_IN_RELEASE: &[&str] = &[
    "dev-policy-key",
    "dev-signing-key",
    "dev-weak-rng",
    "test-hooks",
    "maintenance-mode",
    "qemu-dev-https",
    "phase02-terminal-profile",
    "phase03-filter-profile",
    "phase04-driver-profile",
    "getrandom-sas-test",
    "x86-idt-cpl3-test",
    "x86-mmio-smoke",
    "ipc-trace",
    "cell-scale-experiment",
];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Posture {
    Dev,
    Test,
    Release,
}

impl Posture {
    pub const fn as_str(self) -> &'static str {
        match self {
            Posture::Dev => "dev",
            Posture::Test => "test",
            Posture::Release => "release",
        }
    }
}

/// Derive the posture from `target_arch` and a feature query (`on("test-hooks")`).
///
/// Returns `Err(reason)` for a combination that must not build.
pub fn derive(target_arch: &str, on: &dyn Fn(&str) -> bool) -> Result<Posture, &'static str> {
    let wide = matches!(target_arch, "x86_64" | "aarch64" | "riscv64");
    if wide && !on("native-domains") && !on("compile-only") {
        return Err(
            "64-bit kernels require native-domains (raw-memcpy user copy otherwise); \
                    use compile-only for a build that never boots",
        );
    }
    if !on("release-signing-key") {
        return Ok(if on("test-hooks") {
            Posture::Test
        } else {
            Posture::Dev
        });
    }
    if FORBIDDEN_IN_RELEASE.iter().any(|feature| on(feature)) {
        return Err("release posture cannot be combined with a development, test, recovery \
                    or fixture feature; build with --no-default-features --features release-posture");
    }
    if !on("signing-required") || !on("policy-required") {
        return Err("release posture requires signing-required and policy-required");
    }
    Ok(Posture::Release)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(features: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |f: &str| features.contains(&f)
    }

    const RELEASE: &[&str] = &[
        "release-signing-key",
        "signing-required",
        "policy-required",
        "native-domains",
    ];

    #[test]
    fn release_posture_is_release() {
        assert_eq!(derive("x86_64", &with(RELEASE)), Ok(Posture::Release));
    }

    #[test]
    fn every_forbidden_feature_breaks_a_release_build() {
        for forbidden in FORBIDDEN_IN_RELEASE {
            let set: Vec<&str> = RELEASE.iter().copied().chain([*forbidden]).collect();
            let on = |f: &str| set.contains(&f);
            assert!(derive("x86_64", &on).is_err(), "{forbidden} was accepted");
        }
    }

    #[test]
    fn release_needs_fail_closed_policy_and_signing() {
        let on = |f: &str| f != "policy-required" && RELEASE.contains(&f);
        assert!(derive("riscv64", &on).is_err());
        let on = |f: &str| f != "signing-required" && RELEASE.contains(&f);
        assert!(derive("riscv64", &on).is_err());
    }

    #[test]
    fn wide_targets_need_guarded_copy_unless_compile_only() {
        let on = |f: &str| {
            f == "release-signing-key" || f == "signing-required" || f == "policy-required"
        };
        assert!(derive("aarch64", &on).is_err());
        assert_eq!(
            derive("aarch64", &with(&["compile-only"])),
            Ok(Posture::Dev)
        );
        assert_eq!(derive("arm", &with(&[])), Ok(Posture::Dev));
    }

    #[test]
    fn default_and_test_postures() {
        let default = &[
            "dev-policy-key",
            "dev-signing-key",
            "dev-weak-rng",
            "signing-required",
            "native-domains",
        ];
        assert_eq!(derive("x86_64", &with(default)), Ok(Posture::Dev));
        assert_eq!(
            derive("x86_64", &with(&["native-domains", "test-hooks"])),
            Ok(Posture::Test)
        );
    }
}

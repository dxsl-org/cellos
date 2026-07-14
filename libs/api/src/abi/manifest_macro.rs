//! `declare_manifest!` — embeds a [`super::manifest::CellManifest`] into the
//! current Cell's `__ViCell_manifest` ELF section.  Split out of `manifest.rs`
//! to keep that file under the 200-LOC law.

/// Embed a capability manifest into the current Cell's ELF binary.
///
/// Places a fixed 16-byte `CellManifest` (v2) into the `__ViCell_manifest` ELF
/// section.  The cell linker script must `KEEP` that section or `--gc-sections`
/// will silently drop it in release/LTO builds.
///
/// The back-compat forms default `tier = TIER_LEGACY` (v1 behaviour preserved).
/// Add `tier = <TIER_*>` to opt a cell into an explicit isolation domain (e.g. a
/// Tier-1b C/FFI cell requesting `tier = 2` for PKU key 2).
#[macro_export]
macro_rules! declare_manifest {
    // Full form + explicit tier (v2 opt-in).
    (block_io = $bio:literal, network = $net:literal, spawn = $spawn:literal, gpio = $gpio:literal, uart = $uart:literal, hypervisor = $hv:literal, part_data = $pd:literal, part_lfs = $pl:literal, can = $can:literal, adc = $adc:literal, tier = $tier:expr) => {
        #[used]
        #[link_section = "__ViCell_manifest"]
        pub static VICELL_MANIFEST: $crate::manifest::CellManifest =
            $crate::manifest::CellManifest::with_all(
                $bio, $net, $spawn, $gpio, $uart, $hv, $pd, $pl, $can, $adc, $tier,
            );
    };
    // Full form — block-I/O partition range grants (tier defaults to LEGACY).
    (block_io = $bio:literal, network = $net:literal, spawn = $spawn:literal, gpio = $gpio:literal, uart = $uart:literal, hypervisor = $hv:literal, part_data = $pd:literal, part_lfs = $pl:literal) => {
        #[used]
        #[link_section = "__ViCell_manifest"]
        pub static VICELL_MANIFEST: $crate::manifest::CellManifest =
            $crate::manifest::CellManifest::with_parts(
                $bio, $net, $spawn, $gpio, $uart, $hv, $pd, $pl,
            );
    };
    // Convenience form: block_io with partition scopes, no gpio/uart/hypervisor.
    (block_io = $bio:literal, network = $net:literal, spawn = $spawn:literal, part_data = $pd:literal, part_lfs = $pl:literal) => {
        $crate::declare_manifest!(
            block_io = $bio,
            network = $net,
            spawn = $spawn,
            gpio = false,
            uart = false,
            hypervisor = false,
            part_data = $pd,
            part_lfs = $pl
        );
    };
    // 3-cap form + explicit tier (v2 opt-in), no gpio/uart/hypervisor/parts/can/adc.
    // The pairing `app_entry!` uses for a plain Rust-cell app that wants an
    // explicit isolation domain (e.g. a Tier-1b C/FFI cell requesting tier=2).
    (block_io = $bio:literal, network = $net:literal, spawn = $spawn:literal, tier = $tier:expr) => {
        $crate::declare_manifest!(
            block_io = $bio,
            network = $net,
            spawn = $spawn,
            gpio = false,
            uart = false,
            hypervisor = false,
            part_data = false,
            part_lfs = false,
            can = false,
            adc = false,
            tier = $tier
        );
    };
    // 6-param form — includes hypervisor flag.
    (block_io = $bio:literal, network = $net:literal, spawn = $spawn:literal, gpio = $gpio:literal, uart = $uart:literal, hypervisor = $hv:literal) => {
        #[used]
        #[link_section = "__ViCell_manifest"]
        pub static VICELL_MANIFEST: $crate::manifest::CellManifest =
            $crate::manifest::CellManifest::new($bio, $net, $spawn, $gpio, $uart, $hv);
    };
    // 5-param form (no hypervisor) — hypervisor defaults to false.
    (block_io = $bio:literal, network = $net:literal, spawn = $spawn:literal, gpio = $gpio:literal, uart = $uart:literal) => {
        $crate::declare_manifest!(
            block_io = $bio,
            network = $net,
            spawn = $spawn,
            gpio = $gpio,
            uart = $uart,
            hypervisor = false
        );
    };
    // 3-param back-compat form (no gpio/uart/hypervisor) — all default to false.
    (block_io = $bio:literal, network = $net:literal, spawn = $spawn:literal) => {
        $crate::declare_manifest!(
            block_io = $bio,
            network = $net,
            spawn = $spawn,
            gpio = false,
            uart = false,
            hypervisor = false
        );
    };
}

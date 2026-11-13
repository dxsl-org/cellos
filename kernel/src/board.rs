#[cfg(cellos_wide)]
use cellos_boards::{Architecture, BoardDescriptor, ValidationError};

#[cfg(all(target_arch = "riscv64", feature = "board-pioneer"))]
const SELECTED_RISCV64_BOARD: &BoardDescriptor = &cellos_boards::milk_v_pioneer::MILK_V_PIONEER;
#[cfg(all(
    target_arch = "riscv64",
    not(feature = "board-pioneer"),
    feature = "board-vf2"
))]
const SELECTED_RISCV64_BOARD: &BoardDescriptor =
    &cellos_boards::starfive_visionfive_2::STARFIVE_VISIONFIVE_2;
#[cfg(all(
    target_arch = "riscv64",
    not(feature = "board-pioneer"),
    not(feature = "board-vf2")
))]
const SELECTED_RISCV64_BOARD: &BoardDescriptor =
    &cellos_boards::qemu_virt_riscv64::QEMU_VIRT_RISCV64;

#[cfg(target_arch = "riscv64")]
/// Returns the descriptor the compatibility board feature compiled in, for
/// const boot data; everything else reads [`active`].
pub(crate) const fn compiled() -> &'static BoardDescriptor {
    SELECTED_RISCV64_BOARD
}

#[cfg(target_arch = "riscv64")]
/// Returns the validated descriptor before early boot consumes MMIO or RAM ranges.
pub(crate) fn active() -> &'static BoardDescriptor {
    match SELECTED_RISCV64_BOARD.validate_for(Architecture::Riscv64) {
        Ok(()) => SELECTED_RISCV64_BOARD,
        Err(error) => invalid_descriptor(error),
    }
}

#[cfg(cellos_wide)]
fn invalid_descriptor(error: ValidationError) -> ! {
    panic!("[board] invalid descriptor: {:?}", error)
}

/// Slug bound when the command line names no profile. It must be the strict
/// (`DmaIsolation::Required`) catalog entry; a host test pins that.
#[cfg(target_arch = "x86_64")]
pub(crate) const DEFAULT_X86_PROFILE: &str = "x86_64-pc";

/// Longest kernel command line accepted from Limine, in bytes.
#[cfg(target_arch = "x86_64")]
pub(crate) const CMDLINE_MAX: usize = 1024;

#[cfg(target_arch = "x86_64")]
const PROFILE_KEY: &str = "cellos.profile";

#[cfg(target_arch = "x86_64")]
const PROFILE_SLUG_MAX: usize = 64;

/// Write-once holder of the profile bound at boot.
#[cfg(target_arch = "x86_64")]
struct ProfileCell(core::sync::atomic::AtomicPtr<BoardDescriptor>);

#[cfg(target_arch = "x86_64")]
impl ProfileCell {
    const fn new() -> Self {
        Self(core::sync::atomic::AtomicPtr::new(core::ptr::null_mut()))
    }

    /// Stores `board` if nothing is bound yet; `false` if a profile already is.
    fn bind(&self, board: &'static BoardDescriptor) -> bool {
        use core::sync::atomic::Ordering;
        self.0
            .compare_exchange(
                core::ptr::null_mut(),
                (board as *const BoardDescriptor).cast_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn get(&self) -> Option<&'static BoardDescriptor> {
        let board = self.0.load(core::sync::atomic::Ordering::Acquire);
        // SAFETY: `bind` is the only store, and it stores only pointers taken
        // from `&'static BoardDescriptor`; the pointee is never written.
        unsafe { board.as_ref() }
    }
}

#[cfg(target_arch = "x86_64")]
static BOUND_X86_PROFILE: ProfileCell = ProfileCell::new();

/// Boot facts kept after binding for the machine fingerprint (syscall 433).
/// Written once by [`bind_x86_profile`] together with the profile; read by
/// [`x86_machine_inputs`].
#[cfg(target_arch = "x86_64")]
static X86_MACHINE_FACTS: crate::sync::Spinlock<Option<X86BootFacts>> =
    crate::sync::Spinlock::ranked(None, crate::sync::rank::X86_MACHINE_FACTS);

/// The bound profile and the boot facts recorded with it, or `None` before
/// binding. Never panics: syscall 433 may be asked before or without a profile.
#[cfg(target_arch = "x86_64")]
pub(crate) fn x86_machine_inputs() -> Option<(&'static BoardDescriptor, X86BootFacts)> {
    let facts = (*X86_MACHINE_FACTS.lock())?;
    Some((BOUND_X86_PROFILE.get()?, facts))
}

/// The x86 profile bound at boot by [`bind_x86_profile`].
///
/// # Panics
/// Before binding. Code that runs before ACPI discovery reads
/// `hal_soc_x86::X86_EARLY_FACTS` instead.
#[cfg(target_arch = "x86_64")]
pub(crate) fn active() -> &'static BoardDescriptor {
    match BOUND_X86_PROFILE.get() {
        Some(board) => board,
        None => panic!("[board] x86 profile read before boot bound one"),
    }
}

/// The platform profile an x86 catalog descriptor pairs with.
#[cfg(target_arch = "x86_64")]
pub(crate) fn x86_soc_profile(board: &BoardDescriptor) -> &'static hal_soc_x86::X86PlatformProfile {
    use cellos_boards::SocId;

    let profile = match board.soc {
        SocId::QemuX86Q35 => &hal_soc_x86::QEMU_Q35,
        SocId::GenericX86Pc => &hal_soc_x86::GENERIC_X86_PC,
        SocId::GenericX86Vm => &hal_soc_x86::GENERIC_X86_VM,
        _ => panic!("[board] x86 descriptor has incompatible SoC identity"),
    };
    match profile.validate() {
        Ok(()) => profile,
        Err(error) => panic!("[board] invalid x86 SoC profile: {:?}", error),
    }
}

/// Why the boot command line was refused.
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CmdlineError {
    /// Longer than [`CMDLINE_MAX`].
    TooLong,
    /// A byte outside printable ASCII and tab.
    NotPrintableAscii,
    /// `cellos.profile` with an empty value, or a value that is not a slug
    /// (`[a-z0-9_-]`, at most 64 bytes).
    InvalidProfile,
    /// `cellos.profile` given more than once.
    DuplicateProfile,
}

/// The `cellos.profile=` value on `cmdline`, or `None` when it is absent.
///
/// Tokens are separated by spaces or tabs. Every other token is ignored, but
/// the whole line must be printable ASCII and at most [`CMDLINE_MAX`] bytes.
#[cfg(target_arch = "x86_64")]
pub(crate) fn profile_option(cmdline: &[u8]) -> Result<Option<&str>, CmdlineError> {
    if cmdline.len() > CMDLINE_MAX {
        return Err(CmdlineError::TooLong);
    }
    if !cmdline
        .iter()
        .all(|byte| matches!(byte, b'\t' | 0x20..=0x7E))
    {
        return Err(CmdlineError::NotPrintableAscii);
    }
    let text = core::str::from_utf8(cmdline).map_err(|_| CmdlineError::NotPrintableAscii)?;
    let mut found = None;
    for token in text.split([' ', '\t']).filter(|token| !token.is_empty()) {
        let value = match token.split_once('=') {
            Some((PROFILE_KEY, value)) => value,
            None if token == PROFILE_KEY => "",
            _ => continue,
        };
        let is_slug = !value.is_empty()
            && value.len() <= PROFILE_SLUG_MAX
            && value
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'));
        if !is_slug {
            return Err(CmdlineError::InvalidProfile);
        }
        if found.replace(value).is_some() {
            return Err(CmdlineError::DuplicateProfile);
        }
    }
    Ok(found)
}

#[cfg(target_arch = "x86_64")]
const CONSOLE_KEY: &str = "cellos.console";

/// The `cellos.console=` value on `cmdline`, or `None` when it is absent.
#[cfg(target_arch = "x86_64")]
pub(crate) fn console_option(cmdline: &[u8]) -> Result<Option<&str>, CmdlineError> {
    if cmdline.len() > CMDLINE_MAX {
        return Err(CmdlineError::TooLong);
    }
    if !cmdline
        .iter()
        .all(|byte| matches!(byte, b'\t' | 0x20..=0x7E))
    {
        return Err(CmdlineError::NotPrintableAscii);
    }
    let text = core::str::from_utf8(cmdline).map_err(|_| CmdlineError::NotPrintableAscii)?;
    let mut found = None;
    for token in text.split([' ', '\t']).filter(|token| !token.is_empty()) {
        let value = match token.split_once('=') {
            Some((CONSOLE_KEY, value)) => value,
            None if token == CONSOLE_KEY => "",
            _ => continue,
        };
        if value.is_empty() || value.len() > 64 {
            continue;
        }
        if found.replace(value).is_some() {
            return Err(CmdlineError::DuplicateProfile);
        }
    }
    Ok(found)
}

/// Where the bound profile came from.
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProfileSource {
    CommandLine,
    Default,
}

/// Why no x86 profile could be bound. Every variant stops boot.
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BindError<'a> {
    Cmdline(CmdlineError),
    /// The named slug is not in `cellos_boards::x86_catalog()`.
    UnknownProfile(&'a str),
    /// The named profile weakens DMA isolation, and CPUID reports no
    /// hypervisor to make that consistent with the machine.
    NeedsHypervisor(&'static str),
}

#[cfg(target_arch = "x86_64")]
impl core::fmt::Display for BindError<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Cmdline(error) => write!(f, "boot command line refused: {:?}", error),
            Self::UnknownProfile(slug) => {
                write!(f, "unknown cellos.profile={slug}; this image binds only:")?;
                for board in cellos_boards::x86_catalog() {
                    write!(f, " {}", board.slug)?;
                }
                Ok(())
            }
            Self::NeedsHypervisor(slug) => write!(
                f,
                "cellos.profile={slug} does not require DMA isolation and is admitted only \
                 under a hypervisor, but CPUID reports none"
            ),
        }
    }
}

/// Applies the x86 binding rules (ADR-0029 §2) to the boot command line and
/// the detected identity.
///
/// 1. A `cellos.profile=` slug selects that catalog entry; an unknown slug is
///    an error.
/// 2. Without one, [`DEFAULT_X86_PROFILE`] applies.
/// 3. A profile that does not require DMA isolation is refused unless CPUID
///    reports a hypervisor. Detected facts only reject; they never choose.
#[cfg(target_arch = "x86_64")]
pub(crate) fn choose_x86_profile<'a>(
    cmdline: &'a [u8],
    identity: &PlatformIdentity,
) -> Result<(&'static BoardDescriptor, ProfileSource), BindError<'a>> {
    let (slug, source) = match profile_option(cmdline).map_err(BindError::Cmdline)? {
        Some(slug) => (slug, ProfileSource::CommandLine),
        None => (DEFAULT_X86_PROFILE, ProfileSource::Default),
    };
    let board = cellos_boards::find_by_slug(cellos_boards::x86_catalog(), slug)
        .ok_or(BindError::UnknownProfile(slug))?;
    if board.dma_isolation != cellos_boards::DmaIsolation::Required && !identity.hypervisor {
        return Err(BindError::NeedsHypervisor(board.slug));
    }
    Ok((board, source))
}

/// Binds `board` as the x86 profile for the rest of the boot, keeps `facts`
/// for the machine fingerprint, and names the profile on the console.
///
/// # Panics
/// If `board` fails validation or a profile is already bound.
#[cfg(target_arch = "x86_64")]
pub(crate) fn bind_x86_profile(board: &'static BoardDescriptor, facts: X86BootFacts) {
    if let Err(error) = board.validate_for(Architecture::X86_64) {
        invalid_descriptor(error);
    }
    let soc = x86_soc_profile(board);
    if !board.has_driver(cellos_boards::DriverId::Uart16550PortIo) {
        panic!(
            "[board] x86 profile {} does not enable the early UART",
            board.slug
        );
    }
    {
        let mut kept = X86_MACHINE_FACTS.lock();
        if kept.is_some() || !BOUND_X86_PROFILE.bind(board) {
            panic!("[board] x86 profile bound twice");
        }
        *kept = Some(facts);
    }
    let source = facts.source;
    log::info!(
        "[x86-gate] board={} soc-profile={} source={} dma_isolation={:?}",
        board.slug,
        soc.slug,
        match source {
            ProfileSource::CommandLine => "cmdline",
            ProfileSource::Default => "default",
        },
        board.dma_isolation
    );
    // A weaker-than-Required profile is only reachable by naming it under a
    // hypervisor (`choose_x86_profile`); say so on the console in operator terms.
    if board.dma_isolation != cellos_boards::DmaIsolation::Required {
        log::warn!("[x86-gate] dma isolation optional (operator-selected VM profile)");
    }
}

/// Machine identity facts detected at boot on x86_64.
///
/// Plain data: it selects nothing and grants nothing. Profile binding may use
/// it only to validate or reject a profile the operator named (ADR-0029 §2).
/// Every byte string in it is firmware-controlled.
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlatformIdentity {
    /// OEM fields of the validated ACPI root table; `None` without one.
    pub(crate) acpi_oem: Option<crate::acpi::AcpiOem>,
    /// SMBIOS type 1; `None` when firmware publishes none or it is malformed.
    pub(crate) smbios: Option<crate::smbios::SmbiosSystem>,
    /// `CPUID.1:ECX[31]`: a hypervisor reports itself.
    pub(crate) hypervisor: bool,
}

/// What binding keeps for the machine fingerprint. Plain data: it selects
/// nothing and grants nothing.
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct X86BootFacts {
    pub(crate) identity: PlatformIdentity,
    pub(crate) source: ProfileSource,
    /// Firmware published a DMAR table (`AcpiInfo::dmar_present`).
    pub(crate) dmar_present: bool,
}

/// One log line: `acpi_oem=… smbios=… hv=0|1`. Firmware strings are printed
/// quoted, with bytes outside printable ASCII, `"` and `\` escaped, so they
/// cannot break the line or forge another record.
#[cfg(target_arch = "x86_64")]
impl core::fmt::Display for PlatformIdentity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.acpi_oem {
            Some(oem) => write!(
                f,
                "acpi_oem={} acpi_table={} acpi_rev={:#x}",
                Quoted(&oem.oem_id),
                Quoted(&oem.oem_table_id),
                oem.oem_revision
            )?,
            None => f.write_str("acpi_oem=none")?,
        }
        match &self.smbios {
            Some(system) => {
                write!(
                    f,
                    " smbios=manufacturer={} product={} uuid=",
                    Quoted(system.manufacturer.as_bytes()),
                    Quoted(system.product.as_bytes())
                )?;
                match system.uuid {
                    Some(uuid) => {
                        for (index, byte) in uuid.iter().enumerate() {
                            if matches!(index, 4 | 6 | 8 | 10) {
                                f.write_str("-")?;
                            }
                            write!(f, "{:02x}", byte)?;
                        }
                    }
                    None => f.write_str("none")?,
                }
            }
            None => f.write_str(" smbios=none")?,
        }
        write!(f, " hv={}", u8::from(self.hypervisor))
    }
}

/// Logs `[identity] <facts>`.
#[cfg(target_arch = "x86_64")]
pub(crate) fn report_identity(identity: &PlatformIdentity) {
    log::info!("[identity] {identity}");
}

/// Firmware bytes rendered as a quoted, escaped ASCII string.
#[cfg(target_arch = "x86_64")]
struct Quoted<'a>(&'a [u8]);

#[cfg(target_arch = "x86_64")]
impl core::fmt::Display for Quoted<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("\"")?;
        for &byte in self.0 {
            match byte {
                b'"' | b'\\' => write!(f, "\\{}", byte as char)?,
                0x20..=0x7E => write!(f, "{}", byte as char)?,
                _ => write!(f, "\\x{:02x}", byte)?,
            }
        }
        f.write_str("\"")
    }
}

/// The AArch64 board descriptor, selected by the HAL board backend.
///
/// The HAL cannot name `cellos_boards` without a manifest change, so the
/// backend's SoC identity picks the descriptor here (Group 1: one board per SoC).
/// The hosted VM backend shares QEMU virt's SoC identity (same device map and
/// MMIO authority table), so its feature names the descriptor directly.
#[cfg(all(target_arch = "aarch64", feature = "board-aarch64-vm"))]
const AARCH64_BOARD: &BoardDescriptor = &cellos_boards::vm_aarch64::AARCH64_VM;
#[cfg(all(target_arch = "aarch64", not(feature = "board-aarch64-vm")))]
const AARCH64_BOARD: &BoardDescriptor =
    match <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::SOC {
        hal::aarch64::backend::ArmSoc::QemuArmVirt => {
            &cellos_boards::qemu_virt_aarch64::QEMU_VIRT_AARCH64
        }
        hal::aarch64::backend::ArmSoc::Bcm2837 => {
            &cellos_boards::raspberry_pi_3_model_b::RASPBERRY_PI_3_MODEL_B
        }
        hal::aarch64::backend::ArmSoc::Bcm2711 => {
            &cellos_boards::raspberry_pi_4_model_b::RASPBERRY_PI_4_MODEL_B
        }
        hal::aarch64::backend::ArmSoc::AppleT8103 => &cellos_boards::apple_macmini_m1::MACMINI_M1,
    };

#[cfg(all(
    feature = "board-aarch64-vm",
    any(
        feature = "board-rpi3",
        feature = "board-rpi4",
        feature = "board-apple-t8103"
    )
))]
compile_error!(
    "Conflicting AArch64 board features: `board-aarch64-vm` cannot be combined with another \
     AArch64 board feature."
);

#[cfg(target_arch = "aarch64")]
/// Returns the compiled-in AArch64 descriptor, for const fallback boot data;
/// everything else reads [`active`].
pub(crate) const fn compiled() -> &'static BoardDescriptor {
    AARCH64_BOARD
}

#[cfg(target_arch = "aarch64")]
/// Returns the validated AArch64 descriptor before platform drivers consume it.
pub(crate) fn active() -> &'static BoardDescriptor {
    match AARCH64_BOARD.validate_for(Architecture::Aarch64) {
        Ok(()) => AARCH64_BOARD,
        Err(error) => invalid_descriptor(error),
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::{
        choose_x86_profile, profile_option, x86_soc_profile, BindError, CmdlineError,
        PlatformIdentity, ProfileCell, ProfileSource, CMDLINE_MAX, DEFAULT_X86_PROFILE,
    };
    use crate::acpi::AcpiOem;
    use crate::smbios::{SmbiosString, SmbiosSystem};
    use alloc::format;
    use cellos_boards::{DmaIsolation, DriverId};

    #[test]
    fn identity_line_names_every_fact() {
        let identity = PlatformIdentity {
            acpi_oem: Some(AcpiOem {
                oem_id: *b"BOCHS ",
                oem_table_id: *b"BXPC    ",
                oem_revision: 1,
            }),
            smbios: Some(SmbiosSystem {
                manufacturer: SmbiosString::new(b"QEMU"),
                product: SmbiosString::new(b"Standard PC (Q35 + ICH9, 2009)"),
                uuid: Some([
                    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC,
                    0xDD, 0xEE, 0xFF,
                ]),
            }),
            hypervisor: true,
        };
        assert_eq!(
            format!("{identity}"),
            "acpi_oem=\"BOCHS \" acpi_table=\"BXPC    \" acpi_rev=0x1 \
             smbios=manufacturer=\"QEMU\" product=\"Standard PC (Q35 + ICH9, 2009)\" \
             uuid=00112233-4455-6677-8899-aabbccddeeff hv=1"
        );
    }

    #[test]
    fn identity_line_escapes_firmware_bytes_and_names_absence() {
        let identity = PlatformIdentity {
            acpi_oem: None,
            smbios: Some(SmbiosSystem {
                manufacturer: SmbiosString::new(b"a\"b\\c\n[x] {}%s\xff"),
                product: SmbiosString::EMPTY,
                uuid: None,
            }),
            hypervisor: false,
        };
        assert_eq!(
            format!("{identity}"),
            "acpi_oem=none smbios=manufacturer=\"a\\\"b\\\\c\\x0a[x] {}%s\\xff\" product=\"\" \
             uuid=none hv=0"
        );
        let none = PlatformIdentity {
            acpi_oem: None,
            smbios: None,
            hypervisor: false,
        };
        assert_eq!(format!("{none}"), "acpi_oem=none smbios=none hv=0");
    }

    fn facts(hypervisor: bool) -> PlatformIdentity {
        PlatformIdentity {
            acpi_oem: None,
            smbios: None,
            hypervisor,
        }
    }

    #[test]
    fn profile_option_reads_one_bounded_slug_and_ignores_other_keys() {
        assert_eq!(profile_option(b""), Ok(None));
        assert_eq!(profile_option(b"  quiet\tconsole=ttyS0 "), Ok(None));
        assert_eq!(
            profile_option(b"quiet cellos.profile=qemu-q35-x86_64\tdebug"),
            Ok(Some("qemu-q35-x86_64"))
        );
        // Keys are exact and case-sensitive; look-alikes are other keys.
        assert_eq!(
            profile_option(b"CELLOS.PROFILE=x cellos.profiles=y"),
            Ok(None)
        );
        for (cmdline, error) in [
            (&b"cellos.profile="[..], CmdlineError::InvalidProfile),
            (b"cellos.profile", CmdlineError::InvalidProfile),
            (b"cellos.profile=Q35", CmdlineError::InvalidProfile),
            (b"cellos.profile=a=b", CmdlineError::InvalidProfile),
            (
                b"cellos.profile=x86_64-pc,q35",
                CmdlineError::InvalidProfile,
            ),
            (
                b"cellos.profile=x86_64-pc cellos.profile=x86_64-pc",
                CmdlineError::DuplicateProfile,
            ),
            (
                b"cellos.profile=x86_64-pc\n",
                CmdlineError::NotPrintableAscii,
            ),
            (b"quiet \xff", CmdlineError::NotPrintableAscii),
        ] {
            assert_eq!(profile_option(cmdline), Err(error), "{cmdline:?}");
        }
        let slug = alloc::format!("cellos.profile={}", "a".repeat(65));
        assert_eq!(
            profile_option(slug.as_bytes()),
            Err(CmdlineError::InvalidProfile)
        );
        let mut long = alloc::vec![b' '; CMDLINE_MAX];
        assert_eq!(profile_option(&long), Ok(None));
        long.push(b' ');
        assert_eq!(profile_option(&long), Err(CmdlineError::TooLong));
    }

    #[test]
    fn binding_rules_default_strict_and_never_let_facts_choose() {
        // No option: the strict profile, whatever was detected.
        for hypervisor in [false, true] {
            let (board, source) = choose_x86_profile(b"quiet", &facts(hypervisor)).unwrap();
            assert_eq!(board.slug, DEFAULT_X86_PROFILE);
            assert_eq!(board.dma_isolation, DmaIsolation::Required);
            assert_eq!(source, ProfileSource::Default);
        }
        // The weak profile needs both the operator's choice and a hypervisor.
        let (board, source) =
            choose_x86_profile(b"cellos.profile=qemu-q35-x86_64", &facts(true)).unwrap();
        assert_eq!(board.slug, "qemu-q35-x86_64");
        assert_eq!(source, ProfileSource::CommandLine);
        assert_eq!(
            choose_x86_profile(b"cellos.profile=qemu-q35-x86_64", &facts(false)),
            Err(BindError::NeedsHypervisor("qemu-q35-x86_64"))
        );
        // ADR-0031 D-VM: the generic VM profile follows the same rule.
        let (board, _) = choose_x86_profile(b"cellos.profile=x86_64-vm", &facts(true)).unwrap();
        assert_eq!(board.slug, "x86_64-vm");
        assert_eq!(board.dma_isolation, DmaIsolation::Optional);
        assert_eq!(
            choose_x86_profile(b"cellos.profile=x86_64-vm", &facts(false)),
            Err(BindError::NeedsHypervisor("x86_64-vm"))
        );
        // Naming the strict profile needs no detected fact.
        let (board, source) =
            choose_x86_profile(b"cellos.profile=x86_64-pc", &facts(false)).unwrap();
        assert_eq!(board.slug, "x86_64-pc");
        assert_eq!(source, ProfileSource::CommandLine);
        // Unknown and malformed selections fail; they never fall back.
        assert_eq!(
            choose_x86_profile(b"cellos.profile=qemu-virt-riscv64", &facts(true)),
            Err(BindError::UnknownProfile("qemu-virt-riscv64"))
        );
        assert_eq!(
            choose_x86_profile(b"cellos.profile=", &facts(true)),
            Err(BindError::Cmdline(CmdlineError::InvalidProfile))
        );
    }

    #[test]
    fn bind_errors_name_the_refusal() {
        assert_eq!(
            format!("{}", BindError::UnknownProfile("pc")),
            "unknown cellos.profile=pc; this image binds only: x86_64-pc qemu-q35-x86_64 x86_64-vm"
        );
        assert!(format!("{}", BindError::NeedsHypervisor("qemu-q35-x86_64"))
            .contains("admitted only under a hypervisor"));
        assert_eq!(
            format!("{}", BindError::Cmdline(CmdlineError::TooLong)),
            "boot command line refused: TooLong"
        );
    }

    #[test]
    fn every_catalog_profile_pairs_with_an_early_facts_soc_profile() {
        for board in cellos_boards::x86_catalog() {
            let soc = x86_soc_profile(board);
            assert_eq!(soc.slug, board.slug);
            assert!(hal_soc_x86::X86_PROFILES
                .iter()
                .any(|profile| **profile == *soc));
            assert!(soc.has_early_facts(&hal_soc_x86::X86_EARLY_FACTS));
            assert!(
                board.has_driver(DriverId::Uart16550PortIo),
                "{}",
                board.slug
            );
        }
    }

    #[test]
    fn the_profile_cell_binds_once() {
        let cell = ProfileCell::new();
        assert!(cell.get().is_none());
        assert!(cell.bind(cellos_boards::pc_x86_64()));
        assert!(!cell.bind(cellos_boards::qemu_q35_x86_64()));
        assert_eq!(cell.get().map(|board| board.slug), Some("x86_64-pc"));
    }
}

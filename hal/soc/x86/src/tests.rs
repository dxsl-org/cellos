use super::*;

#[test]
fn qemu_q35_preserves_the_verified_legacy_contract() {
    assert_eq!(QEMU_Q35.validate(), Ok(()));
    assert_eq!(QEMU_Q35.com1.base, 0x03F8);
    assert_eq!(QEMU_Q35.com1.irq, 4);
    assert!(QEMU_Q35.legacy_bios_window.contains(0x0008_0000, 16));
    assert!(!QEMU_Q35.legacy_bios_window.contains(0x0007_FFFF, 16));
    assert!(QEMU_Q35.legacy_rsdp_window.contains(0x000E_0000, 36));
}

#[test]
fn generic_pc_profile_reuses_the_standard_legacy_wiring() {
    assert_eq!(GENERIC_X86_PC.validate(), Ok(()));
    assert_eq!(GENERIC_X86_PC.slug, "x86_64-pc");
    assert_eq!(GENERIC_X86_PC.com1.base, 0x03F8);
    assert_eq!(GENERIC_X86_PC.com1.irq, 4);
    assert!(GENERIC_X86_PC.legacy_bios_window.contains(0x0008_0000, 16));
    assert!(GENERIC_X86_PC.legacy_rsdp_window.contains(0x000E_0000, 36));
    assert_ne!(GENERIC_X86_PC.slug, QEMU_Q35.slug);
}

#[test]
fn firmware_windows_reject_overflow_and_out_of_range_access() {
    let overflowing = AddressRange {
        base: usize::MAX - 1,
        size: 4,
    };
    assert_eq!(overflowing.end(), None);
    assert!(!overflowing.contains(usize::MAX - 1, 1));
    assert!(!QEMU_Q35.legacy_rsdp_window.contains(0x000F_FFF0, 32));
}

#[test]
fn both_profiles_declare_the_console_as_the_first_of_their_ports() {
    for profile in [QEMU_Q35, GENERIC_X86_PC] {
        assert_eq!(profile.validate(), Ok(()));
        assert_eq!(profile.serial_ports.len(), 4);
        assert_eq!(profile.serial_ports[0], profile.com1);
        // The standard ISA layout, and every port distinct.
        assert_eq!(profile.serial_ports[1].base, 0x02F8);
        assert_eq!(profile.serial_ports[2].base, 0x03E8);
        assert_eq!(profile.serial_ports[3].base, 0x02E8);
        assert_eq!(profile.serial_port(0), Some(profile.com1));
        assert_eq!(profile.serial_port(4), None);
        // Nothing claims a transceiver on a machine that has none.
        assert!(profile.rs485.is_empty());
    }
}

fn test_profile(
    serial_ports: &'static [PortIoDevice],
    rs485: &'static [Rs485Port],
) -> X86PlatformProfile {
    X86PlatformProfile {
        slug: "test",
        com1: PortIoDevice {
            base: 0x03F8,
            irq: 4,
        },
        serial_ports,
        rs485,
        legacy_bios_window: AddressRange {
            base: 0x0008_0000,
            size: 0x0008_0000,
        },
        legacy_rsdp_window: AddressRange {
            base: 0,
            size: 0x0010_0000,
        },
    }
}

#[test]
fn validation_rejects_malformed_serial_declarations() {
    static CONSOLE_LAST: [PortIoDevice; 2] = [
        PortIoDevice {
            base: 0x02F8,
            irq: 3,
        },
        PortIoDevice {
            base: 0x03F8,
            irq: 4,
        },
    ];
    assert_eq!(
        test_profile(&CONSOLE_LAST, &[]).validate(),
        Err(ValidationError::ConsoleNotFirstSerialPort)
    );

    static DUPLICATE: [PortIoDevice; 2] = [
        PortIoDevice {
            base: 0x03F8,
            irq: 4,
        },
        PortIoDevice {
            base: 0x03F8,
            irq: 3,
        },
    ];
    assert_eq!(
        test_profile(&DUPLICATE, &[]).validate(),
        Err(ValidationError::DuplicateSerialPort)
    );

    assert_eq!(
        test_profile(&[], &[]).validate(),
        Err(ValidationError::NoSerialPorts)
    );

    // A profile whose console IS first, so the RS485 checks are the ones reached.
    static CONSOLE_FIRST: [PortIoDevice; 2] = [
        PortIoDevice {
            base: 0x03F8,
            irq: 4,
        },
        PortIoDevice {
            base: 0x02F8,
            irq: 3,
        },
    ];
    static RS485_RANGE: [Rs485Port; 1] = [Rs485Port {
        port: 3,
        direction: Rs485Direction::RtsAuto,
        turnaround_ns: 500,
    }];
    assert_eq!(
        test_profile(&CONSOLE_FIRST, &RS485_RANGE).validate(),
        Err(ValidationError::Rs485PortOutOfRange)
    );

    static RS485_ZERO: [Rs485Port; 1] = [Rs485Port {
        port: 1,
        direction: Rs485Direction::Gpio { offset: 4 },
        turnaround_ns: 0,
    }];
    assert_eq!(
        test_profile(&CONSOLE_FIRST, &RS485_ZERO).validate(),
        Err(ValidationError::Rs485ZeroTurnaround)
    );

    // A well-formed declaration validates: the mechanism is expressible even
    // though no machine claims it yet.
    static RS485_OK: [Rs485Port; 1] = [Rs485Port {
        port: 1,
        direction: Rs485Direction::Gpio { offset: 4 },
        turnaround_ns: 750,
    }];
    assert_eq!(test_profile(&CONSOLE_FIRST, &RS485_OK).validate(), Ok(()));
}

//! Intel X710/XL710 register map and MMIO trait.

#![allow(dead_code)]

pub(crate) const PF_ATQBAL: usize = 0x0008_0000;
pub(crate) const PF_ATQBAH: usize = 0x0008_0100;
pub(crate) const PF_ATQLEN: usize = 0x0008_0200;
pub(crate) const PF_ATQH: usize = 0x0008_0300;
pub(crate) const PF_ATQT: usize = 0x0008_0400;

pub(crate) const PF_ARQBAL: usize = 0x0008_0080;
pub(crate) const PF_ARQBAH: usize = 0x0008_0180;
pub(crate) const PF_ARQLEN: usize = 0x0008_0280;
pub(crate) const PF_ARQH: usize = 0x0008_0380;
pub(crate) const PF_ARQT: usize = 0x0008_0480;

pub(crate) const GLGEN_RST: usize = 0x000B_8180;
pub(crate) const GLGEN_RST_DEV_ARM: u32 = 0x0000_0001;

pub(crate) const PF_ATQLEN_ENABLE: u32 = 1 << 31;
pub(crate) const PF_ARQLEN_ENABLE: u32 = 1 << 31;

pub(crate) const QTX_TAIL: usize = 0x0010_0000;
pub(crate) const QRX_TAIL: usize = 0x0012_A000;

pub(crate) use nic_driver::regs::RegisterFile;

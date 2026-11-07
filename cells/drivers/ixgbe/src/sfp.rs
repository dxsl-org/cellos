//! Intel 82599 SFP+ I2C bit-banging and module identification (SFF-8472).
//!
//! Intel 82599 10 GbE Controller Datasheet §8.2.3.24.

#![allow(dead_code)]

use crate::regs::{RegisterFile, I2CCTL_82599, I2C_CLK_OUT, I2C_DATA_IN, I2C_DATA_OUT};
use types::{ViError, ViResult};

pub(crate) const SFP_I2C_ADDR: u8 = 0xA0; // SFF-8472 2-wire serial address
pub(crate) const SFP_TYPE_SFP_PLUS: u8 = 0x03;

fn i2c_set_clk(regs: &impl RegisterFile, high: bool) -> ViResult<()> {
    let mut val = regs.rd32(I2CCTL_82599)?;
    if high {
        val |= I2C_CLK_OUT;
    } else {
        val &= !I2C_CLK_OUT;
    }
    regs.wr32(I2CCTL_82599, val)
}

fn i2c_set_data(regs: &impl RegisterFile, high: bool) -> ViResult<()> {
    let mut val = regs.rd32(I2CCTL_82599)?;
    if high {
        val |= I2C_DATA_OUT;
    } else {
        val &= !I2C_DATA_OUT;
    }
    regs.wr32(I2CCTL_82599, val)
}

fn i2c_get_data(regs: &impl RegisterFile) -> ViResult<bool> {
    let val = regs.rd32(I2CCTL_82599)?;
    Ok(val & I2C_DATA_IN != 0)
}

fn i2c_start(regs: &impl RegisterFile) -> ViResult<()> {
    i2c_set_data(regs, true)?;
    i2c_set_clk(regs, true)?;
    i2c_set_data(regs, false)?;
    i2c_set_clk(regs, false)
}

fn i2c_stop(regs: &impl RegisterFile) -> ViResult<()> {
    i2c_set_data(regs, false)?;
    i2c_set_clk(regs, true)?;
    i2c_set_data(regs, true)
}

fn i2c_write_byte(regs: &impl RegisterFile, byte: u8) -> ViResult<bool> {
    for i in (0..8).rev() {
        let bit = (byte >> i) & 1 != 0;
        i2c_set_data(regs, bit)?;
        i2c_set_clk(regs, true)?;
        i2c_set_clk(regs, false)?;
    }
    // Read ACK
    i2c_set_data(regs, true)?;
    i2c_set_clk(regs, true)?;
    let ack = !i2c_get_data(regs)?;
    i2c_set_clk(regs, false)?;
    Ok(ack)
}

fn i2c_read_byte(regs: &impl RegisterFile, ack: bool) -> ViResult<u8> {
    let mut byte = 0u8;
    i2c_set_data(regs, true)?;
    for _ in 0..8 {
        i2c_set_clk(regs, true)?;
        byte = (byte << 1) | (i2c_get_data(regs)? as u8);
        i2c_set_clk(regs, false)?;
    }
    // Send ACK / NACK
    i2c_set_data(regs, !ack)?;
    i2c_set_clk(regs, true)?;
    i2c_set_clk(regs, false)?;
    Ok(byte)
}

/// Read a single byte from SFP module EEPROM at `offset`.
pub(crate) fn read_sfp_byte(regs: &impl RegisterFile, offset: u8) -> ViResult<u8> {
    i2c_start(regs)?;
    if !i2c_write_byte(regs, SFP_I2C_ADDR)? {
        i2c_stop(regs)?;
        return Err(ViError::IO);
    }
    if !i2c_write_byte(regs, offset)? {
        i2c_stop(regs)?;
        return Err(ViError::IO);
    }
    i2c_start(regs)?;
    if !i2c_write_byte(regs, SFP_I2C_ADDR | 1)? {
        i2c_stop(regs)?;
        return Err(ViError::IO);
    }
    let data = i2c_read_byte(regs, false)?;
    i2c_stop(regs)?;
    Ok(data)
}

/// Identify whether a valid SFP/SFP+ transceiver is present.
pub(crate) fn identify_sfp(regs: &impl RegisterFile) -> ViResult<bool> {
    match read_sfp_byte(regs, 0) {
        Ok(id) => Ok(id == SFP_TYPE_SFP_PLUS),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sfp_i2c_address_matches_sff_8472() {
        assert_eq!(SFP_I2C_ADDR, 0xA0);
        assert_eq!(SFP_TYPE_SFP_PLUS, 0x03);
    }
}

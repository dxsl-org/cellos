#![no_std]

/// SPI bus error kinds.
#[derive(Debug, PartialEq)]
pub enum SpiError {
    /// GPIO or bus-state fault (pin direction, MMIO error).
    BusError,
    /// Transfer aborted mid-frame (reserved for future hardware SPI adapters).
    TransferError,
}

/// Synchronous SPI master trait (Mode 0: CPOL=0, CPHA=0).
///
/// # Contract
/// - Implementations shift **MSB-first**.
/// - `transfer` and `write` manage chip-select internally (CS asserted before
///   first clock, deasserted after last clock or on error).
/// - `cs_select` / `cs_deselect` are provided for multi-transfer transactions
///   where the caller needs to keep CS asserted across multiple `transfer`/`write`
///   calls (advanced use — most callers should use `transfer`/`write` directly).
/// - `transfer`: clocks `max(tx.len(), rx.len())` SCK pulses. TX bytes beyond
///   `tx.len()` are padded with `0x00`; RX bytes beyond `rx.len()` are discarded.
/// - All methods are synchronous (no async boundary) so `&[u8]` / `&mut [u8]` are valid.
pub trait ViSpi {
    type Error: core::fmt::Debug;

    /// Assert chip-select (active-low).
    ///
    /// For advanced multi-transfer use only; `transfer`/`write` assert CS automatically.
    fn cs_select(&mut self) -> Result<(), Self::Error>;

    /// Deassert chip-select.
    ///
    /// For advanced multi-transfer use only; `transfer`/`write` deassert CS automatically.
    fn cs_deselect(&mut self) -> Result<(), Self::Error>;

    /// Full-duplex transfer: shift out `tx[i]`, shift in `rx[i]` simultaneously.
    ///
    /// Asserts CS before the first clock and deasserts it after the last clock (or on error).
    /// Clocks `max(tx.len(), rx.len())` SCK pulses.
    /// TX bytes beyond `tx.len()` are driven as `0x00`.
    /// RX bytes beyond `rx.len()` are discarded (MISO still clocked in but dropped).
    fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> Result<(), Self::Error>;

    /// Write-only transfer: shift out every byte in `bytes`, discard MISO.
    ///
    /// Asserts CS before the first clock and deasserts it after the last clock (or on error).
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

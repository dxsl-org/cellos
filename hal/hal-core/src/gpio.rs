use crate::HalResult;

/// Represents a Digital Input/Output Pin
pub trait GpioPin {
    /// Configure the pin as Input or Output
    fn set_mode(&mut self, mode: PinMode) -> HalResult<()>;
    
    /// Write High (True) or Low (False)
    fn write(&mut self, state: bool) -> HalResult<()>;
    
    /// Read the current state
    fn read(&self) -> HalResult<bool>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinMode {
    Input,
    Output,
    Alternate(u8),
}

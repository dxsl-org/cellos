use alloc::string::String;
use crate::syscall::sys_read;

pub struct Stdin;

pub fn stdin() -> Stdin {
    Stdin
}

impl Stdin {
    pub fn read_line(&self, buffer: &mut String) -> Result<usize, ()> {
        loop {
            let mut b = [0u8; 1];
            // sys_read(0, ...) maps to Stdin in Kernel
            match sys_read(0, &mut b) {
                Ok(n) if n > 0 => {
                    let c = b[0] as char;
                    
                    // Handle Enter
                    if c == '\n' || c == '\r' {
                        crate::print!("\n");
                        break;
                    }
                    
                    // Handle Backspace (0x08 or 0x7F)
                    if c == '\x08' || c == '\x7F' {
                        if !buffer.is_empty() {
                            buffer.pop();
                            // Erase on screen: Backspace, Space, Backspace
                            crate::print!("\x08 \x08");
                        }
                        continue;
                    }
                    
                    // Normal Printable Char
                    // We assume ASCII for now.
                    buffer.push(c);
                    crate::print!("{}", c);
                }
                _ => {
                    // Start of logic error: sys_read(0) should block.
                    // If it returns 0 or Err, something failed.
                    // We can yield just in case.
                    crate::syscall::sys_yield();
                }
            }
        }
        Ok(buffer.len())
    }
}

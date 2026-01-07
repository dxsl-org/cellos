use core::arch::global_asm;

global_asm!(include_str!("asm/switch.S"));
global_asm!(include_str!("asm/trap.S"));

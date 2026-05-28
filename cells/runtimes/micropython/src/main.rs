#![no_std]
#![no_main]

extern crate ostd;
extern crate api;
extern crate cty;

use ostd::prelude::*;

// FFI to MicroPython
extern "C" {
    fn mp_init();
    fn mp_deinit();
    
    // We might need to manually trigger execution of a script or REPL
    // function pyexec_friendly_repl() is standard for repl
    fn pyexec_friendly_repl() -> cty::c_int;
}

// Memory area for MicroPython Heap
static mut HEAP: [u8; 128 * 1024] = [0; 128 * 1024]; // 128KB heap

#[no_mangle]
pub extern "C" fn main(_argc: isize, _argv: *const *const u8) -> isize {
    println!("MicroPython v1.24.1 on ViOS");

    unsafe {
        // Initialize GC heap
        // mp_stack_ctrl_init(); // optional
        
        // gc_init(heap, heap + len)
        extern "C" {
            fn gc_init(start: *mut cty::c_void, end: *mut cty::c_void);
        }
        
        let heap_ptr = HEAP.as_mut_ptr() as *mut cty::c_void;
        let heap_end = heap_ptr.add(HEAP.len());
        gc_init(heap_ptr, heap_end);

        mp_init();
        
        // Execute REPL
        pyexec_friendly_repl();
        
        mp_deinit();
    }
    
    0
}

// Callbacks required by MicroPython runtime that might not be in POSIX shim
#[no_mangle]
pub extern "C" fn nlr_jump_fail(_val: *mut cty::c_void) {
    panic!("MicroPython NLR Jump Fail");
}

#![no_std]
#![no_main]

use core::{arch::asm, panic::PanicInfo};

const AT_FDCWD: usize = (-100isize) as usize;
const O_RDWR: usize = 2;
const SYS_IOCTL: usize = 29;
const SYS_OPENAT: usize = 56;
const SYS_CLOSE: usize = 57;
const SYS_WRITE: usize = 64;
const SYS_EXIT: usize = 93;
const FBIOGET_VSCREENINFO: usize = 0x4600;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let card = open(b"/dev/dri/card0\0");
    if card < 0 {
        print(b"TIER3B_T1_CARD0_FAIL\n");
        exit(1);
    }
    close(card as usize);
    print(b"TIER3B_T1_CARD0_OK\n");

    let framebuffer = open(b"/dev/fb0\0");
    if framebuffer < 0 {
        print(b"TIER3B_T2_FB_LIFECYCLE_FAIL\n");
        exit(2);
    }
    let mut info = [0u8; 160];
    let ioctl_result = unsafe {
        syscall3(
            SYS_IOCTL,
            framebuffer as usize,
            FBIOGET_VSCREENINFO,
            info.as_mut_ptr() as usize,
        )
    };
    if ioctl_result < 0 {
        print(b"TIER3B_T2_FB_LIFECYCLE_FAIL\n");
        exit(3);
    }
    let width = read_u32(&info, 0) as usize;
    let height = read_u32(&info, 4) as usize;
    let bpp = read_u32(&info, 24);
    let xrgb = bpp == 32
        && read_u32(&info, 32) == 16
        && read_u32(&info, 44) == 8
        && read_u32(&info, 56) == 0;

    let mut pixels = [0u8; 4096];
    let chunk_len = pixels.len();
    let target = width
        .checked_mul(height)
        .and_then(|count| count.checked_mul(4))
        .unwrap_or(0)
        .min(8 * 1024 * 1024);
    let mut written = 0usize;
    while written < target {
        fill_pattern(&mut pixels, written / chunk_len);
        let count = (target - written).min(pixels.len());
        let result = unsafe {
            syscall3(
                SYS_WRITE,
                framebuffer as usize,
                pixels.as_ptr() as usize,
                count,
            )
        };
        if result <= 0 {
            break;
        }
        written += result as usize;
    }
    close(framebuffer as usize);
    if written == 0 {
        print(b"TIER3B_T2_FB_LIFECYCLE_FAIL\n");
        exit(4);
    }
    print(b"TIER3B_T2_FB_LIFECYCLE_OK\n");
    if xrgb {
        print(b"TIER3B_T12_XRGB_SCANOUT_OK\n");
    } else {
        print(b"TIER3B_T12_XRGB_SCANOUT_FAIL\n");
        exit(5);
    }
    exit(0)
}

fn fill_pattern(buffer: &mut [u8], band: usize) {
    let color = match band % 3 {
        0 => [0x20, 0x20, 0xe0, 0xff],
        1 => [0x20, 0xe0, 0x20, 0xff],
        _ => [0xe0, 0x20, 0x20, 0xff],
    };
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.copy_from_slice(&color);
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap_or([0; 4]))
}

fn open(path: &[u8]) -> isize {
    unsafe { syscall4(SYS_OPENAT, AT_FDCWD, path.as_ptr() as usize, O_RDWR, 0) }
}

fn close(fd: usize) {
    unsafe {
        syscall3(SYS_CLOSE, fd, 0, 0);
    }
}

fn print(message: &[u8]) {
    unsafe {
        syscall3(SYS_WRITE, 1, message.as_ptr() as usize, message.len());
    }
}

fn exit(code: usize) -> ! {
    unsafe {
        syscall3(SYS_EXIT, code, 0, 0);
    }
    loop {
        core::hint::spin_loop();
    }
}

unsafe fn syscall3(number: usize, a0: usize, a1: usize, a2: usize) -> isize {
    syscall4(number, a0, a1, a2, 0)
}

unsafe fn syscall4(number: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> isize {
    let result: usize;
    asm!(
        "svc 0",
        in("x8") number,
        inlateout("x0") a0 => result,
        in("x1") a1,
        in("x2") a2,
        in("x3") a3,
        options(nostack),
    );
    result as isize
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(127)
}

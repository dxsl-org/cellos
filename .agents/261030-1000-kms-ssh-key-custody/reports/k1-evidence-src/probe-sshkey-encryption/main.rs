#![no_std]
#![no_main]
extern crate alloc;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::hint::black_box;
use core::sync::atomic::{AtomicUsize, Ordering};

const ARENA: usize = 256 * 1024;
struct Bump { mem: UnsafeCell<[u8; ARENA]>, next: AtomicUsize }
unsafe impl Sync for Bump {}
unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let mut cur = self.next.load(Ordering::Relaxed);
        loop {
            let start = (cur + l.align() - 1) & !(l.align() - 1);
            let end = start + l.size();
            if end > ARENA { return core::ptr::null_mut(); }
            match self.next.compare_exchange(cur, end, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return unsafe { (self.mem.get() as *mut u8).add(start) },
                Err(c) => cur = c,
            }
        }
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static A: Bump = Bump { mem: UnsafeCell::new([0; ARENA]), next: AtomicUsize::new(0) };

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }

static PEM: [u8; 600] = [0x41; 600];
static PASS: [u8; 8] = *b"passw0rd";
static MSG: [u8; 80] = [7; 80];

use ed25519_dalek::{Signer, SigningKey};
use ssh_key::{Cipher, Kdf, PrivateKey};

fn run(pem: &[u8], pass: &[u8], msg: &[u8]) -> Option<[u8; 64]> {
    let k = PrivateKey::from_openssh(pem).ok()?;
    if k.cipher() != Cipher::Aes256Ctr { return None; }
    match k.kdf() {
        Kdf::Bcrypt { salt, rounds } if (16..=64).contains(&salt.len()) && (1..=64).contains(rounds) => {}
        _ => return None,
    }
    let d = k.decrypt(pass).ok()?;
    let ed = d.key_data().ed25519()?;
    let sk = SigningKey::from_bytes(&ed.private.to_bytes());
    if sk.verifying_key().to_bytes() != ed.public.0 { return None; }
    Some(sk.sign(msg).to_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let r = run(black_box(&PEM), black_box(&PASS), black_box(&MSG));
    black_box(r);
    loop {}
}

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

use aes::Aes256;
use aes::cipher::{KeyIvInit, StreamCipher};
use alloc::vec::Vec;
use base64ct::{Base64, Encoding};
use bcrypt_pbkdf::bcrypt_pbkdf_with_memory;
use ctr::Ctr128BE;
use ed25519_dalek::{Signer, SigningKey};
use zeroize::Zeroizing;

const MAGIC: &[u8] = b"openssh-key-v1\0";
const MAX_FILE: usize = 64 * 1024;

struct R<'a>(&'a [u8]);
impl<'a> R<'a> {
    fn u32(&mut self) -> Option<u32> {
        let (h, t) = self.0.split_first_chunk::<4>()?;
        self.0 = t;
        Some(u32::from_be_bytes(*h))
    }
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n { return None; }
        let (h, t) = self.0.split_at(n);
        self.0 = t;
        Some(h)
    }
    fn string(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.bytes(n)
    }
}

fn run(pem: &[u8], pass: &[u8], msg: &[u8]) -> Option<[u8; 64]> {
    if pem.len() > MAX_FILE { return None; }
    let s = core::str::from_utf8(pem).ok()?;
    let body: Vec<u8> = s
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .flat_map(|l| l.bytes())
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let bin = Zeroizing::new(Base64::decode_vec(core::str::from_utf8(&body).ok()?).ok()?);
    let mut r = R(&bin);
    if r.bytes(MAGIC.len())? != MAGIC { return None; }
    // ---- allowlist before ANY expensive work ----
    if r.string()? != b"aes256-ctr" { return None; }
    if r.string()? != b"bcrypt" { return None; }
    let mut opts = R(r.string()?);
    let salt = opts.string()?;
    let rounds = opts.u32()?;
    if !opts.0.is_empty() || !(16..=64).contains(&salt.len()) || !(1..=64).contains(&rounds) { return None; }
    if r.u32()? != 1 { return None; }
    let pubblob = r.string()?;
    let ct = r.string()?;
    if !r.0.is_empty() || ct.is_empty() || ct.len() % 16 != 0 || ct.len() > MAX_FILE { return None; }
    // ---- KDF + decrypt ----
    let mut okm = Zeroizing::new([0u8; 64]);
    let mut mem = Zeroizing::new([0u8; 64]);
    bcrypt_pbkdf_with_memory(pass, salt, rounds, &mut okm[..48], &mut mem[..]).ok()?;
    let mut buf = Zeroizing::new(ct.to_vec());
    let mut c = Ctr128BE::<Aes256>::new_from_slices(&okm[..32], &okm[32..48]).ok()?;
    c.apply_keystream(&mut buf);
    let mut p = R(&buf);
    let c1 = p.u32()?;
    let c2 = p.u32()?;
    if c1 != c2 { return None; }
    if p.string()? != b"ssh-ed25519" { return None; }
    let pubk = p.string()?;
    let kp = p.string()?;
    let _comment = p.string()?;
    // padding 1,2,3,... (< 16)
    if p.0.len() >= 16 || p.0.iter().enumerate().any(|(i, b)| *b as usize != i + 1) { return None; }
    if pubk.len() != 32 || kp.len() != 64 || &kp[32..] != pubk { return None; }
    let mut pb = R(pubblob);
    if pb.string()? != b"ssh-ed25519" || pb.string()? != pubk { return None; }
    let seed: &[u8; 32] = kp[..32].try_into().ok()?;
    let sk = SigningKey::from_bytes(seed);
    if sk.verifying_key().as_bytes()[..] != *pubk { return None; }
    Some(sk.sign(msg).to_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let r = run(black_box(&PEM), black_box(&PASS), black_box(&MSG));
    black_box(r);
    loop {}
}

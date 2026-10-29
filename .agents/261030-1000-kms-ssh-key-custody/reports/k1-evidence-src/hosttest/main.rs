use aes::Aes256;
use aes::cipher::{KeyIvInit, StreamCipher};
extern crate alloc;
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


use std::time::Instant;
use ssh_key::{Cipher, Kdf, PrivateKey};

fn time_kdf(rounds: u32) -> f64 {
    let mut out = [0u8; 48];
    let mut mem = [0u8; 64];
    let t = Instant::now();
    bcrypt_pbkdf::bcrypt_pbkdf_with_memory(b"hunter42", &[0x5a; 16], rounds, &mut out, &mut mem).unwrap();
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let enc = std::fs::read("fixtures/id_ed25519.aes256-ctr.enc").unwrap();
    let cha = std::fs::read("fixtures/id_ed25519.chacha20-poly1305.enc").unwrap();
    let a128 = std::fs::read("fixtures/id_ed25519.aes128-ctr.enc").unwrap();
    let msg = b"k1 probe message";
    let pubhex = "b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62";
    // own minimal parser
    let sig = run(&enc, b"hunter42", msg).expect("ownmin decrypt+sign");
    let vk_bytes: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&pubhex[2*i..2*i+2], 16).unwrap()).collect();
    let vk = ed25519_dalek::VerifyingKey::from_bytes(vk_bytes.as_slice().try_into().unwrap()).unwrap();
    let s = ed25519_dalek::Signature::from_bytes(&sig);
    println!("ownmin: sign ok, verify against fixture pubkey = {}", vk.verify_strict(msg, &s).is_ok());
    println!("ownmin: wrong passphrase rejected = {}", run(&enc, b"hunter43", msg).is_none());
    println!("ownmin: chacha20-poly1305 fixture rejected pre-KDF = {}", run(&cha, b"hunter42", msg).is_none());
    println!("ownmin: aes128-ctr fixture rejected pre-KDF = {}", run(&a128, b"hunter42", msg).is_none());
    // ssh-key + encryption
    let k = PrivateKey::from_openssh(&enc).unwrap();
    let d = k.decrypt(b"hunter42").unwrap();
    let seed = d.key_data().ed25519().unwrap().private.to_bytes();
    let sk = ed25519_dalek::SigningKey::from_bytes(&seed);
    use ed25519_dalek::Signer;
    println!("ssh-key+encryption: same signature as ownmin = {}", sk.sign(msg).to_bytes() == sig);
    println!("ssh-key+encryption: wrong passphrase error = {:?}", k.decrypt(b"hunter43").err());
    // pre-KDF inspection works without any KDF work
    let kc = PrivateKey::from_openssh(&cha).unwrap();
    println!("ssh-key: chacha fixture cipher()={:?} kdf rounds readable pre-decrypt: {:?}", kc.cipher(), match kc.kdf() { Kdf::Bcrypt{rounds,salt} => Some((*rounds, salt.len())), _ => None });
    // hostile rounds: patch the kdfoptions rounds u32 of the aes256-ctr fixture
    use base64ct::{Base64, Encoding};
    let s = std::str::from_utf8(&enc).unwrap();
    let b64: String = s.lines().filter(|l| !l.starts_with("-----")).collect();
    let mut bin = Base64::decode_vec(&b64).unwrap();
    // magic(15) + str(4+10 aes256-ctr) + str(4+6 bcrypt) + u32 optlen(4) + salt str(4+16) => rounds at 15+14+10+4+20
    let off = 15 + 14 + 10 + 4 + 20;
    println!("fixture rounds before patch = {}", u32::from_be_bytes(bin[off..off+4].try_into().unwrap()));
    bin[off..off+4].copy_from_slice(&0x7fff_ffffu32.to_be_bytes());
    let kh = PrivateKey::from_bytes(&bin).unwrap();
    println!("ssh-key: hostile rounds=0x7fffffff PARSES OK (no cap); kdf = rounds {:?} (decrypt() would run the KDF unbounded)", match kh.kdf() { Kdf::Bcrypt{rounds,..} => Some(*rounds), _ => None });
    // ownmin rejection of the same bytes (re-encode)
    let mut pem = String::from("-----BEGIN OPENSSH PRIVATE KEY-----\n");
    pem.push_str(&Base64::encode_string(&bin)); pem.push_str("\n-----END OPENSSH PRIVATE KEY-----\n");
    let t = Instant::now();
    println!("ownmin: hostile rounds rejected = {} in {:?}", run(pem.as_bytes(), b"hunter42", msg).is_none(), t.elapsed());
    // bcrypt-pbkdf API contract
    let mut o48 = [0u8; 48]; let mut m = [0u8; 64];
    let r = |p: &[u8], s: &[u8], rounds: u32, o: &mut [u8], m: &mut [u8]| bcrypt_pbkdf::bcrypt_pbkdf_with_memory(p, s, rounds, o, m);
    println!("api rounds=0   -> {:?}", r(b"x", &[1;16], 0, &mut o48, &mut m));
    println!("api rounds=1   -> {:?}", r(b"x", &[1;16], 1, &mut o48, &mut m));
    println!("api rounds=64  -> {:?}", r(b"x", &[1;16], 64, &mut o48, &mut m));
    println!("api rounds=65  -> {:?}", r(b"x", &[1;16], 65, &mut o48, &mut m));
    println!("api salt len 0 -> {:?}", r(b"x", &[], 1, &mut o48, &mut m));
    println!("api salt len 15-> {:?}", r(b"x", &[1;15], 1, &mut o48, &mut m));
    println!("api salt len 64-> {:?}", r(b"x", &[1;64], 1, &mut o48, &mut m));
    println!("api salt len 65-> {:?}", r(b"x", &[1;65], 1, &mut o48, &mut m));
    println!("api passphrase empty -> {:?}", r(b"", &[1;16], 1, &mut o48, &mut m));
    println!("api passphrase 1024 B -> {:?}", r(&[b'a'; 1024], &[1;16], 1, &mut o48, &mut m));
    let mut o1025 = vec![0u8; 1025]; let mut big = vec![0u8; 2048];
    println!("api output 1025 -> {:?}", r(b"x", &[1;16], 1, &mut o1025, &mut big));
    let mut short = [0u8; 32];
    println!("api memory 32 for 48 out -> {:?}", r(b"x", &[1;16], 1, &mut o48, &mut short));
    // timings (host, release, this machine)
    for rounds in [1u32, 16, 64] {
        let mut v: Vec<f64> = (0..5).map(|_| time_kdf(rounds)).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("host timing bcrypt_pbkdf 48B rounds={rounds}: min {:.2} ms median {:.2} ms max {:.2} ms", v[0], v[2], v[4]);
    }
}

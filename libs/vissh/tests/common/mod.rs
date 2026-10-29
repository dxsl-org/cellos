//! In-memory wiring for Sunset runners plus plain-packet helpers for scripted peers.
#![allow(dead_code)]

use std::io::Read;

use vissh::sunset::{self, Runner};

pub fn os_entropy(buf: &mut [u8]) -> bool {
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf)).is_ok()
}

/// Bytes in flight in one direction. A runner refuses input while its own KEX output
/// is pending, so both directions are buffered instead of copied runner to runner.
#[derive(Default)]
pub struct Wire(pub Vec<u8>);

impl Wire {
    /// Take every pending output byte of `from`.
    pub fn take<C: sunset::CliServ>(&mut self, from: &mut Runner<'_, C>) {
        self.0.extend(drain(from));
    }

    /// Give `to` as many buffered bytes as it accepts.
    pub fn give<C: sunset::CliServ>(&mut self, to: &mut Runner<'_, C>) -> sunset::Result<()> {
        while !self.0.is_empty() {
            let n = to.input(&self.0)?;
            if n == 0 {
                break;
            }
            self.0.drain(..n);
        }
        Ok(())
    }
}

/// Drain a runner's output into a byte vector.
pub fn drain<C: sunset::CliServ>(r: &mut Runner<'_, C>) -> Vec<u8> {
    let mut all = Vec::new();
    loop {
        let data = r.output_buf().to_vec();
        if data.is_empty() {
            return all;
        }
        r.consume_output(data.len());
        all.extend_from_slice(&data);
    }
}

/// Split `bytes` after the `SSH-2.0-...\r\n` identification line.
pub fn after_ident(bytes: &[u8]) -> (&[u8], &[u8]) {
    let end = bytes.windows(2).position(|w| w == b"\r\n").expect("ident line") + 2;
    bytes.split_at(end)
}

/// The ten name-lists of the first plaintext KEXINIT packet in `packets`.
pub fn kexinit_lists(packets: &[u8]) -> Vec<Vec<String>> {
    let len = u32::from_be_bytes(packets[..4].try_into().unwrap()) as usize;
    let pad = packets[4] as usize;
    let payload = &packets[5..4 + len - pad];
    assert_eq!(payload[0], 20, "SSH_MSG_KEXINIT");
    let mut at = 17;
    (0..10)
        .map(|_| {
            let n = u32::from_be_bytes(payload[at..at + 4].try_into().unwrap()) as usize;
            let list = std::str::from_utf8(&payload[at + 4..at + 4 + n]).unwrap();
            at += 4 + n;
            if list.is_empty() { Vec::new() } else { list.split(',').map(String::from).collect() }
        })
        .collect()
}

/// A plaintext SSH packet around `payload` (block size 8, at least 4 padding bytes).
pub fn packet(payload: &[u8]) -> Vec<u8> {
    let mut pad = 8 - (payload.len() + 5) % 8;
    if pad < 4 {
        pad += 8;
    }
    let len = (1 + payload.len() + pad) as u32;
    let mut out = len.to_be_bytes().to_vec();
    out.push(pad as u8);
    out.extend_from_slice(payload);
    out.extend(std::iter::repeat_n(0u8, pad));
    out
}

/// A KEXINIT payload with the given lists: kex, hostkey, cipher (both ways), mac (both
/// ways); compression `none`, no languages.
pub fn kexinit(kex: &str, hostkey: &str, cipher: &str, mac: &str) -> Vec<u8> {
    let mut p = vec![20u8];
    p.extend_from_slice(&[7u8; 16]);
    for list in [kex, hostkey, cipher, cipher, mac, mac, "none", "none", "", ""] {
        p.extend_from_slice(&(list.len() as u32).to_be_bytes());
        p.extend_from_slice(list.as_bytes());
    }
    p.push(0);
    p.extend_from_slice(&0u32.to_be_bytes());
    p
}

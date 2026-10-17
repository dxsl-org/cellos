//! Symmetric Cipher abstraction for SSH (ChaCha20-Poly1305 and Plaintext).

#![forbid(unsafe_code)]

extern crate alloc;
use alloc::vec::Vec;
use chacha20::cipher::generic_array::GenericArray;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use chacha20::ChaCha20Legacy;

pub enum CipherMode {
    Plaintext,
    ChaCha20Poly1305 {
        // C2S keys
        c2s_k1: [u8; 32], // payload
        c2s_k2: [u8; 32], // length
        c2s_seq: u32,
        // S2C keys
        s2c_k1: [u8; 32], // payload
        s2c_k2: [u8; 32], // length
        s2c_seq: u32,
    },
}

impl CipherMode {
    pub fn new_plaintext() -> Self {
        Self::Plaintext
    }

    pub fn new_chacha20(c2s_key: &[u8], s2c_key: &[u8], c2s_seq: u32, s2c_seq: u32) -> Self {
        let mut c2s_main = [0u8; 32];
        let mut c2s_header = [0u8; 32];
        let mut s2c_main = [0u8; 32];
        let mut s2c_header = [0u8; 32];

        // OpenSSH chachapoly_new: key[0..32] is main_ctx (payload), key[32..64] is header_ctx (length)
        c2s_main.copy_from_slice(&c2s_key[0..32]);
        c2s_header.copy_from_slice(&c2s_key[32..64]);

        s2c_main.copy_from_slice(&s2c_key[0..32]);
        s2c_header.copy_from_slice(&s2c_key[32..64]);

        Self::ChaCha20Poly1305 {
            c2s_k1: c2s_header, // header / length
            c2s_k2: c2s_main,   // main / payload
            c2s_seq,
            s2c_k1: s2c_header, // header / length
            s2c_k2: s2c_main,   // main / payload
            s2c_seq,
        }
    }

    /// Pack and encrypt an outbound packet to client (S2C)
    pub fn pack_outbound(&mut self, payload: &[u8]) -> Vec<u8> {
        match self {
            Self::Plaintext => {
                let block_size = 8;
                let min_padding = 4;
                let total_unpadded = 4 + 1 + payload.len() + min_padding;
                let rem = total_unpadded % block_size;
                let pad_len = min_padding + if rem == 0 { 0 } else { block_size - rem };
                let packet_length = 1 + payload.len() + pad_len;

                let mut out = Vec::with_capacity(4 + packet_length);
                out.extend_from_slice(&(packet_length as u32).to_be_bytes());
                out.push(pad_len as u8);
                out.extend_from_slice(payload);
                out.resize(out.len() + pad_len, 0);
                out
            }
            Self::ChaCha20Poly1305 { s2c_k1, s2c_k2, s2c_seq, .. } => {
                let block_size = 8;
                let min_padding = 4;
                let unpadded = 1 + payload.len() + min_padding;
                let rem = unpadded % block_size;
                let pad_len = min_padding + if rem == 0 { 0 } else { block_size - rem };
                let packet_length = 1 + payload.len() + pad_len;
                let seq = *s2c_seq;
                *s2c_seq = s2c_seq.wrapping_add(1);

                // Build unencrypted packet: packet_length (4) + padding_len (1) + payload + padding (pad_len)
                let mut pkt = Vec::with_capacity(4 + packet_length);
                pkt.extend_from_slice(&(packet_length as u32).to_be_bytes());
                pkt.push(pad_len as u8);
                pkt.extend_from_slice(payload);
                pkt.resize(pkt.len() + pad_len, 0);

                // OpenSSH ChaCha20-Poly1305 packet format:
                // Nonce for 64-bit ChaCha20 = packet sequence number (u64 BE, 8 bytes)
                let nonce = (seq as u64).to_be_bytes();

                // Encrypt length with K1 (length key per RFC 4253 / PROTOCOL.chacha20poly1305)
                let mut cipher_k1 = ChaCha20Legacy::new(GenericArray::from_slice(s2c_k1), GenericArray::from_slice(&nonce));
                let mut enc_len = [0u8; 4];
                enc_len.copy_from_slice(&pkt[0..4]);
                cipher_k1.apply_keystream(&mut enc_len);

                // Derive Poly1305 key using K2 with counter 0
                let mut cipher_k2 = ChaCha20Legacy::new(GenericArray::from_slice(s2c_k2), GenericArray::from_slice(&nonce));
                let mut poly_key = [0u8; 64];
                cipher_k2.apply_keystream(&mut poly_key);

                // Encrypt body (offset 4..end) with K2 starting at counter 1
                let body = &mut pkt[4..];
                cipher_k2.apply_keystream(body);

                // Compute Poly1305 tag over enc_len || enc_body
                let poly = chacha20poly1305::Tag::default(); // tag calculation
                let mut mac_data = Vec::with_capacity(4 + body.len());
                mac_data.extend_from_slice(&enc_len);
                mac_data.extend_from_slice(body);

                // Use poly1305 directly
                use chacha20poly1305::aead::generic_array::GenericArray;
                use poly1305::Poly1305;
                let poly_key_arr = GenericArray::from_slice(&poly_key[0..32]);
                let poly_inst = <Poly1305 as poly1305::universal_hash::KeyInit>::new(poly_key_arr);
                let tag = poly_inst.compute_unpadded(&mac_data);
                let mut out = Vec::with_capacity(4 + body.len() + 16);
                out.extend_from_slice(&enc_len);
                out.extend_from_slice(body);
                out.extend_from_slice(tag.as_slice());
                out
            }
        }
    }

    /// Try to unpack an inbound packet from client (C2S)
    /// Returns Ok(Some((payload, consumed_bytes))) if a full packet was parsed.
    pub fn unpack_inbound(&mut self, buf: &[u8]) -> Result<Option<(Vec<u8>, usize)>, ()> {
        if buf.is_empty() {
            return Ok(None);
        }

        match self {
            Self::Plaintext => {
                if buf.len() < 4 {
                    return Ok(None);
                }
                let packet_length = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
                if packet_length < 2 || packet_length > 65536 {
                    return Err(());
                }
                let total_needed = 4 + packet_length;
                if buf.len() < total_needed {
                    return Ok(None);
                }
                let pad_len = buf[4] as usize;
                if 1 + pad_len > packet_length {
                    return Err(());
                }
                let payload_len = packet_length - 1 - pad_len;
                let payload = buf[5..5 + payload_len].to_vec();
                Ok(Some((payload, total_needed)))
            }
            Self::ChaCha20Poly1305 { c2s_k1, c2s_k2, c2s_seq, .. } => {
                if buf.len() < 4 {
                    return Ok(None);
                }
                let seq = *c2s_seq;

                // Nonce for 64-bit ChaCha20 = packet sequence number (u64 BE, 8 bytes)
                let nonce = (seq as u64).to_be_bytes();

                // Decrypt length with K1 (length key)
                let mut cipher_k1 = ChaCha20Legacy::new(GenericArray::from_slice(c2s_k1), GenericArray::from_slice(&nonce));
                let mut dec_len_bytes = [0u8; 4];
                dec_len_bytes.copy_from_slice(&buf[0..4]);
                cipher_k1.apply_keystream(&mut dec_len_bytes);
                let packet_length = u32::from_be_bytes(dec_len_bytes) as usize;
                if packet_length < 2 || packet_length > 65536 {
                    ostd::io::print("[cipher] invalid len: ");
                    ostd::io::print_usize(packet_length);
                    ostd::io::println("");
                    return Err(());
                }
                let total_needed = 4 + packet_length + 16;
                if buf.len() < total_needed {
                    return Ok(None);
                }

                *c2s_seq = c2s_seq.wrapping_add(1);

                // Compute Poly1305 key with K2 at counter 0
                let mut cipher_k2 = ChaCha20Legacy::new(GenericArray::from_slice(c2s_k2), GenericArray::from_slice(&nonce));
                let mut poly_key = [0u8; 64];
                cipher_k2.apply_keystream(&mut poly_key);

                use chacha20poly1305::aead::generic_array::GenericArray;
                use poly1305::Poly1305;
                let poly_key_arr = GenericArray::from_slice(&poly_key[0..32]);
                let poly_inst = <Poly1305 as poly1305::universal_hash::KeyInit>::new(poly_key_arr);
                let computed_tag = poly_inst.compute_unpadded(&buf[0..4 + packet_length]);
                let actual_tag = &buf[4 + packet_length..total_needed];

                if computed_tag.as_slice() != actual_tag {
                    ostd::io::println("[cipher] MAC mismatch!");
                    return Err(());
                }
                // Decrypt payload with K2 at counter 1
                let mut body = buf[4..4 + packet_length].to_vec();
                cipher_k2.apply_keystream(&mut body);

                let pad_len = body[0] as usize;
                if 1 + pad_len > packet_length {
                    return Err(());
                }
                let payload_len = packet_length - 1 - pad_len;
                let payload = body[1..1 + payload_len].to_vec();
                Ok(Some((payload, total_needed)))
            }
        }
    }
}

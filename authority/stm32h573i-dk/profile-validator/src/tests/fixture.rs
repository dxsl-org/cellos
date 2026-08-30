mod certificate;
mod snapshot;
mod snapshot_flow;
mod snapshot_support;
pub use snapshot::{admitted_snapshot, snapshot};

use certificate::{certificate, key, name, ski, spki, CertificateSpec};
use sha2::{Digest, Sha256};
use std::{vec, vec::Vec};

pub struct Fixture {
    pub profile: Vec<u8>,
    pub root: Vec<u8>,
    pub spki: [u8; 32],
    pub spki_der: Vec<u8>,
    pub node: [u8; 32],
    pub tpm: Vec<u8>,
}

pub fn chain(intermediates: usize) -> Fixture {
    build_chain(intermediates, None)
}

pub fn chain_with_server_auth_intermediate() -> Fixture {
    build_chain(1, Some(&[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01]))
}

fn build_chain(intermediates: usize, ca_eku: Option<&[u8]>) -> Fixture {
    let keys = [key(1), key(2), key(3), key(4)];
    let names = [name(b"leaf"), name(b"ca1"), name(b"ca2"), name(b"root")];
    let spkis = [
        spki(&keys[0]),
        spki(&keys[1]),
        spki(&keys[2]),
        spki(&keys[3]),
    ];
    let skis = [
        ski(&spkis[0]),
        ski(&spkis[1]),
        ski(&spkis[2]),
        ski(&spkis[3]),
    ];
    let root_index = intermediates + 1;
    let root = certificate(CertificateSpec {
        subject: &names[root_index],
        issuer: &names[root_index],
        spki: &spkis[root_index],
        own_ski: &skis[root_index],
        aki: Some(&skis[root_index]),
        signer: &keys[root_index],
        serial: 9,
        ca: true,
        path: Some(intermediates as u8),
        dns: b"node.example",
        node: None,
        ca_eku: None,
    });
    let mut cas: [Option<Vec<u8>>; 2] = [None, None];
    for index in (1..=intermediates).rev() {
        let parent = index + 1;
        cas[index - 1] = Some(certificate(CertificateSpec {
            subject: &names[index],
            issuer: &names[parent],
            spki: &spkis[index],
            own_ski: &skis[index],
            aki: Some(&skis[parent]),
            signer: &keys[parent],
            serial: index as u8 + 1,
            ca: true,
            path: Some((index - 1) as u8),
            dns: b"node.example",
            node: None,
            ca_eku: if index == 1 { ca_eku } else { None },
        }));
    }
    let signing = if intermediates == 0 {
        &keys[root_index]
    } else {
        &keys[1]
    };
    let issuer_name = if intermediates == 0 {
        &names[root_index]
    } else {
        &names[1]
    };
    let issuer_ski = if intermediates == 0 {
        &skis[root_index]
    } else {
        &skis[1]
    };
    let node: [u8; 32] = Sha256::digest(&spkis[0]).into();
    let leaf = certificate(CertificateSpec {
        subject: &names[0],
        issuer: issuer_name,
        spki: &spkis[0],
        own_ski: &skis[0],
        aki: Some(issuer_ski),
        signer: signing,
        serial: 1,
        ca: false,
        path: None,
        dns: b"node.example",
        node: Some(node),
        ca_eku: None,
    });
    let mut profile = leaf;
    for ca in cas.into_iter().take(intermediates) {
        profile.extend(ca.unwrap());
    }
    let tpm = tpm_public(&spkis[0]);
    let spki_digest: [u8; 32] = Sha256::digest(&spkis[0]).into();
    Fixture {
        profile,
        root,
        spki: spki_digest,
        spki_der: spkis[0].clone(),
        node,
        tpm,
    }
}

pub fn unrelated_tpm_public() -> Vec<u8> {
    tpm_public(&spki(&key(9)))
}

fn tpm_public(spki: &[u8]) -> Vec<u8> {
    let point = &spki[26..];
    let mut public = vec![
        0x00, 0x23, 0x00, 0x0b, 0x00, 0x04, 0x00, 0x32, 0x00, 0x00, 0x00, 0x10, 0x00, 0x18, 0x00,
        0x0b, 0x00, 0x03, 0x00, 0x10, 0x00, 0x20,
    ];
    public.extend_from_slice(&point[1..33]);
    public.extend_from_slice(&[0x00, 0x20]);
    public.extend_from_slice(&point[33..]);
    let mut encoded = vec![(public.len() >> 8) as u8, public.len() as u8];
    encoded.extend(public);
    encoded
}

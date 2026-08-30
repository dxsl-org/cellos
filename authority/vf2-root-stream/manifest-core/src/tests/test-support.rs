use crate::*;

pub const SEED: [u8; 32] = [7; 32];
pub const REGION: &[u8] = b"opensbidtbcellosvifs";

pub fn fixture() -> (Manifest, ExpectedManifest, ManifestLimits) {
    let parts: [&[u8]; 4] = [
        &REGION[0..7],
        &REGION[7..10],
        &REGION[10..16],
        &REGION[16..20],
    ];
    let kinds = [
        ComponentKind::OpenSbi,
        ComponentKind::Dtb,
        ComponentKind::Cellos,
        ComponentKind::Vifs,
    ];
    let addresses = [0x8000_0000, 0x8100_0000, 0x8200_0000, 0x8300_0000];
    let mut offset = 0u64;
    let components = core::array::from_fn(|i| {
        let component = Component {
            kind: kinds[i],
            offset,
            length: parts[i].len() as u64,
            load_address: addresses[i],
            sha256: sha256(parts[i]),
        };
        offset += component.length;
        component
    });
    let manifest = Manifest {
        device_id: [1; 32],
        authority_id: [2; 32],
        boot_epoch: 11,
        request_id: 12,
        approved_loader_sha256: [3; 32],
        component_region_length: REGION.len() as u64,
        entry_address: addresses[0],
        components,
    };
    let expected = ExpectedManifest {
        device_id: manifest.device_id,
        authority_id: manifest.authority_id,
        approved_loader_sha256: manifest.approved_loader_sha256,
        boot_epoch: 11,
        request_id: 12,
    };
    let limits = ManifestLimits {
        max_cose_length: MAX_COSE_LEN as u32,
        max_component_region_length: 4096,
        components: core::array::from_fn(|i| ComponentLimit {
            kind: kinds[i],
            load_address: addresses[i],
            max_load_end: addresses[i] + 4096,
            max_size: 4096,
            entry_address: if i == 0 { addresses[0] } else { 0 },
        }),
    };
    (manifest, expected, limits)
}

pub fn signed_cose(manifest: &Manifest) -> (std::vec::Vec<u8>, [u8; 32]) {
    let mut payload = [0u8; MAX_PAYLOAD_LEN];
    let payload_len = encode_payload(manifest, &mut payload).unwrap();
    let mut cose = [0u8; MAX_COSE_LEN];
    let mut scratch = [0u8; MAX_SIG_STRUCTURE_LEN];
    let cose_len = sign_cose(&payload[..payload_len], &SEED, &mut cose, &mut scratch).unwrap();
    (
        cose[..cose_len].to_vec(),
        public_key_from_seed(&SEED).unwrap(),
    )
}

pub fn padded_bundle(manifest: &Manifest) -> (std::vec::Vec<u8>, [u8; 32]) {
    let (cose, public_key) = signed_cose(manifest);
    let logical_len = outer_encoded_len(cose.len(), REGION.len()).unwrap();
    let padded_len = (logical_len + 1023) / 1024 * 1024;
    let mut padded = std::vec![XMODEM_PADDING; padded_len];
    encode_outer(&cose, REGION, &mut padded).unwrap();
    (padded, public_key)
}

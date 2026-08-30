use crate::bundler_args::BundlerArgs;
use crate::cli::CliResult;
use crate::io::{hex, read_bounded, read_seed, ReservedOutputs};
use manifest_core::{
    decode_xmodem, encode_outer, encode_payload, encode_xmodem, outer_encoded_len,
    public_key_from_seed, sha256, sign_cose, validate_manifest, validate_staging, verify_bundle,
    xmodem_encoded_len, Component, ComponentKind, Manifest, EVIDENCE_BOUNDARY, LANE, MAX_COSE_LEN,
    MAX_PAYLOAD_LEN, MAX_SIG_STRUCTURE_LEN,
};
use std::ffi::OsString;

pub fn run<I>(args: I) -> CliResult<()>
where
    I: IntoIterator<Item = OsString>,
{
    let args = BundlerArgs::parse(args)?;
    let manifest_limits = args.common.manifest_limits();
    validate_staging(
        &args.common.staging_limits(),
        &args.common.forbidden,
        &manifest_limits,
    )
    .map_err(core_error)?;
    let outputs = ReservedOutputs::create(&args.transcript_out, &args.summary_out)?;
    let blobs = read_components(&args)?;
    let manifest = make_manifest(&args, &blobs)?;
    validate_manifest(&manifest, &args.common.expected(), &manifest_limits).map_err(core_error)?;

    let mut payload = vec![0; MAX_PAYLOAD_LEN];
    let payload_len = encode_payload(&manifest, &mut payload).map_err(core_error)?;
    payload.truncate(payload_len);
    let mut seed = read_seed(&args.seed)?;
    let public_key = match public_key_from_seed(&seed) {
        Ok(key) => key,
        Err(error) => {
            seed.fill(0);
            return Err(core_error(error));
        }
    };
    let mut cose = vec![0; MAX_COSE_LEN];
    let mut signature_scratch = vec![0; MAX_SIG_STRUCTURE_LEN];
    let signed = sign_cose(&payload, &seed, &mut cose, &mut signature_scratch);
    seed.fill(0);
    let cose_len = signed.map_err(core_error)?;
    signature_scratch.fill(0);
    cose.truncate(cose_len);

    let region = concatenate(&blobs)?;
    let outer_len = outer_encoded_len(cose.len(), region.len()).map_err(core_error)?;
    let mut outer = vec![0; outer_len];
    let encoded_outer = encode_outer(&cose, &region, &mut outer).map_err(core_error)?;
    if encoded_outer != outer_len {
        return Err("manifest-core returned an inconsistent outer length".to_owned());
    }
    let transcript_len =
        xmodem_encoded_len(outer.len(), args.common.max_transfer_blocks).map_err(core_error)?;
    let mut transcript = vec![0; transcript_len];
    let written = encode_xmodem(&outer, &mut transcript, args.common.max_transfer_blocks)
        .map_err(core_error)?;
    if written != transcript_len {
        return Err("manifest-core returned an inconsistent transcript length".to_owned());
    }
    self_verify(&args, &transcript, &public_key)?;
    let summary = summary(&manifest, &public_key, &cose, transcript.len());
    outputs.commit(&transcript, summary.as_bytes())
}

fn read_components(args: &BundlerArgs) -> CliResult<[Vec<u8>; 4]> {
    let labels = ["OpenSBI", "DTB", "Cellos", "VIFS"];
    let mut blobs = Vec::with_capacity(4);
    for (index, path) in args.components.iter().enumerate() {
        let bytes = read_bounded(path, labels[index], args.common.components[index].max_size)?;
        if bytes.is_empty() {
            return Err(format!("{} component must be nonempty", labels[index]));
        }
        if bytes.len() as u64 > args.common.components[index].max_size {
            return Err(format!(
                "{} component exceeds its explicit maximum size",
                labels[index]
            ));
        }
        blobs.push(bytes);
    }
    blobs
        .try_into()
        .map_err(|_| "internal component count error".to_owned())
}

fn make_manifest(args: &BundlerArgs, blobs: &[Vec<u8>; 4]) -> CliResult<Manifest> {
    let kinds = [
        ComponentKind::OpenSbi,
        ComponentKind::Dtb,
        ComponentKind::Cellos,
        ComponentKind::Vifs,
    ];
    let region_length = blobs.iter().try_fold(0u64, |total, blob| {
        let length = u64::try_from(blob.len())
            .map_err(|_| "component length does not fit u64".to_owned())?;
        total
            .checked_add(length)
            .ok_or_else(|| "component region length overflows u64".to_owned())
    })?;
    if region_length > args.common.max_component_region_length {
        return Err("component region exceeds --max-component-region-length".to_owned());
    }
    let mut offset = 0u64;
    let components = core::array::from_fn(|index| {
        let length = blobs[index].len() as u64;
        let component = Component {
            kind: kinds[index],
            offset,
            length,
            load_address: args.common.components[index].load_address,
            sha256: sha256(&blobs[index]),
        };
        offset += length;
        component
    });

    Ok(Manifest {
        device_id: args.common.device_id,
        authority_id: args.common.authority_id,
        boot_epoch: args.common.boot_epoch,
        request_id: args.common.request_id,
        approved_loader_sha256: args.common.approved_loader_sha256,
        component_region_length: region_length,
        entry_address: args.common.entry_address,
        components,
    })
}

fn concatenate(blobs: &[Vec<u8>; 4]) -> CliResult<Vec<u8>> {
    let length = blobs
        .iter()
        .try_fold(0usize, |total, blob| total.checked_add(blob.len()))
        .ok_or_else(|| "component region length overflows usize".to_owned())?;
    let mut region = Vec::with_capacity(length);
    for blob in blobs {
        region.extend_from_slice(blob);
    }
    Ok(region)
}

fn self_verify(args: &BundlerArgs, transcript: &[u8], key: &[u8; 32]) -> CliResult<()> {
    let mut padded = vec![0; transcript.len()];
    let padded_len = decode_xmodem(transcript, &mut padded, args.common.max_transfer_blocks)
        .map_err(core_error)?;
    padded.truncate(padded_len);
    let mut scratch = vec![0; MAX_SIG_STRUCTURE_LEN];
    verify_bundle(
        &padded,
        key,
        &args.common.expected(),
        &args.common.manifest_limits(),
        &mut scratch,
    )
    .map_err(core_error)?;
    Ok(())
}

fn summary(manifest: &Manifest, key: &[u8; 32], cose: &[u8], transcript_len: usize) -> String {
    let names = ["opensbi", "dtb", "cellos", "vifs"];
    let mut text = format!(
        "evidence={EVIDENCE_BOUNDARY}\nlane={LANE}\nproduction_evidence=false\n\
         physical_evidence=false\ndevice_id={}\nauthority_id={}\napproved_loader_sha256={}\n\
         boot_epoch={}\nrequest_id={}\nentry_address=0x{:x}\n\
         component_region_length={}\npublic_key={}\ncose_sha256={}\n\
         transcript_length={}\n",
        hex(&manifest.device_id),
        hex(&manifest.authority_id),
        hex(&manifest.approved_loader_sha256),
        manifest.boot_epoch,
        manifest.request_id,
        manifest.entry_address,
        manifest.component_region_length,
        hex(key),
        hex(&sha256(cose)),
        transcript_len,
    );
    for (name, component) in names.iter().zip(manifest.components.iter()) {
        text.push_str(&format!(
            "component={name} length={} sha256={} load_address=0x{:x}\n",
            component.length,
            hex(&component.sha256),
            component.load_address
        ));
    }
    text
}

fn core_error(error: impl core::fmt::Debug) -> String {
    format!("manifest-core rejected input: {error:?}")
}

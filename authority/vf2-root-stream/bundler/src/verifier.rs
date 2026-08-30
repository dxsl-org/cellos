use crate::cli::CliResult;
use crate::io::{hex, read_bounded, read_public_key};
use crate::verifier_args::VerifierArgs;
use manifest_core::{
    decode_xmodem, validate_staging, verify_bundle, EVIDENCE_BOUNDARY, LANE, MAX_SIG_STRUCTURE_LEN,
    XMODEM_BLOCK_LEN, XMODEM_FRAME_LEN,
};
use std::ffi::OsString;

pub fn run<I>(args: I) -> CliResult<String>
where
    I: IntoIterator<Item = OsString>,
{
    let args = VerifierArgs::parse(args)?;
    let manifest_limits = args.common.manifest_limits();
    validate_staging(
        &args.common.staging_limits(),
        &args.common.forbidden,
        &manifest_limits,
    )
    .map_err(core_error)?;
    let transcript_limit = u64::from(args.common.max_transfer_blocks)
        .checked_mul(XMODEM_FRAME_LEN as u64)
        .and_then(|length| length.checked_add(1))
        .ok_or_else(|| "transcript limit overflows u64".to_owned())?;
    let transcript = read_bounded(&args.transcript, "transcript", transcript_limit)?;
    let public_key = read_public_key(&args.public_key)?;
    let padded_capacity = usize::try_from(args.common.max_transfer_blocks)
        .ok()
        .and_then(|blocks| blocks.checked_mul(XMODEM_BLOCK_LEN))
        .ok_or_else(|| "decoded transcript capacity overflows usize".to_owned())?;
    let mut padded = vec![0; padded_capacity];
    let padded_len = decode_xmodem(&transcript, &mut padded, args.common.max_transfer_blocks)
        .map_err(core_error)?;
    padded.truncate(padded_len);
    let mut signature_scratch = vec![0; MAX_SIG_STRUCTURE_LEN];
    let verified = verify_bundle(
        &padded,
        &public_key,
        &args.common.expected(),
        &manifest_limits,
        &mut signature_scratch,
    )
    .map_err(core_error)?;
    signature_scratch.fill(0);
    Ok(success_report(&verified.manifest))
}

fn success_report(manifest: &manifest_core::Manifest) -> String {
    let names = ["opensbi", "dtb", "cellos", "vifs"];
    let mut report = format!(
        "evidence={EVIDENCE_BOUNDARY}\nlane={LANE}\nproduction_evidence=false\n\
             physical_evidence=false\nstatus=verified\n"
    );
    for (name, component) in names.iter().zip(manifest.components.iter()) {
        report.push_str(&format!(
            "component={name} length={} sha256={}\n",
            component.length,
            hex(&component.sha256)
        ));
    }
    report
}

fn core_error(error: impl core::fmt::Debug) -> String {
    format!("manifest-core rejected input: {error:?}")
}

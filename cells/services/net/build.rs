fn main() {
    cell_build::emit_linker_script();
    emit_min_unix();
    emit_feature_guards();
}

/// Emit `VICELL_MIN_UNIX` — a build-time floor for the TLS clock clamp.
///
/// Uses `SOURCE_DATE_EPOCH` (reproducible-builds standard) when set, otherwise
/// falls back to the fixed constant 1748736000 (2025-06-01T00:00:00Z).
/// This prevents epoch-0 (RTC absent / not yet synced) from causing every
/// certificate to look expired.
///
/// The constant MUST NOT move backward between builds — it is a minimum floor,
/// not the current time.
fn emit_min_unix() {
    // Fixed fallback: 2025-06-01T00:00:00Z
    const FALLBACK: u64 = 1_748_736_000;

    let unix = if let Ok(s) = std::env::var("SOURCE_DATE_EPOCH") {
        s.parse::<u64>().unwrap_or(FALLBACK)
    } else {
        FALLBACK
    };

    println!("cargo:rustc-env=VICELL_MIN_UNIX={unix}");
    // Rebuild only when SOURCE_DATE_EPOCH changes; not sensitive to source edits.
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
}

/// Enforce the mutually exclusive TLS flavors and their trust-store selectors.
///
/// Cargo features unify additively. In particular, QEMU DEV cannot widen the
/// default fleet trust store: it requires disabling the default features.
fn emit_feature_guards() {
    // Detect active features via CARGO_FEATURE_* env vars.
    let roots_embedded = std::env::var("CARGO_FEATURE_TLS_ROOTS_EMBEDDED").is_ok();
    let roots_full = std::env::var("CARGO_FEATURE_TLS_ROOTS_FULL").is_ok();
    let insecure = std::env::var("CARGO_FEATURE_TLS_INSECURE").is_ok();
    let qemu_dev = std::env::var("CARGO_FEATURE_QEMU_DEV_HTTPS").is_ok();
    let ca_private = std::env::var("CARGO_FEATURE_TLS_CA_PRIVATE").is_ok();
    let ca_amazon = std::env::var("CARGO_FEATURE_TLS_CA_AMAZON").is_ok();
    let ca_letsencrypt = std::env::var("CARGO_FEATURE_TLS_CA_LETSENCRYPT").is_ok();
    let ca_rsa = std::env::var("CARGO_FEATURE_TLS_CA_RSA").is_ok();

    let flavor_count = [roots_embedded, roots_full, insecure, qemu_dev]
        .iter()
        .filter(|&&x| x)
        .count();
    let ca_count = [ca_private, ca_amazon, ca_letsencrypt, ca_rsa]
        .iter()
        .filter(|&&x| x)
        .count();

    if flavor_count > 1 {
        panic!(
            "service-net: multiple TLS flavor features active \
             (tls-roots-embedded, tls-roots-full, tls-insecure, qemu-dev-https are mutually exclusive). \
             Specify exactly one."
        );
    }

    // The future full/fleet flavor is still unsupported. QEMU public PKI is
    // deliberately a separate DEV-only selection rather than a fleet cutover.
    let usable_flavor = roots_embedded || insecure || qemu_dev;
    if !usable_flavor {
        panic!(
            "service-net: no usable TLS flavor selected. Choose `tls-roots-embedded` \
             (fleet), `qemu-dev-https` (public DEV), or `tls-insecure` (dev only)."
        );
    }

    if ca_count > 1 {
        panic!(
            "service-net: multiple CA selector features active \
             (tls-ca-private, tls-ca-amazon, tls-ca-letsencrypt, tls-ca-rsa are mutually exclusive). \
             Specify exactly one."
        );
    }

    if insecure && ca_count > 0 {
        panic!(
            "service-net: tls-insecure combined with a CA selector is a configuration error — \
             certs cannot be both verified and unverified. \
             Remove the tls-ca-* feature when using tls-insecure."
        );
    }

    if qemu_dev && ca_count > 0 {
        panic!(
            "service-net: qemu-dev-https uses the public Mozilla trust store and cannot \
             be combined with fleet tls-ca-* selectors. Build with --no-default-features."
        );
    }

    if qemu_dev
        && (std::env::var("CARGO_FEATURE_RAW_RELAY").is_ok()
            || std::env::var("CARGO_FEATURE_K1_FALLBACK").is_ok())
    {
        panic!("service-net: qemu-dev-https excludes raw relay and K1 fallback paths");
    }

    // Rerun when any of these features change (Cargo already does this, but be explicit).
    for var in &[
        "CARGO_FEATURE_TLS_ROOTS_EMBEDDED",
        "CARGO_FEATURE_TLS_ROOTS_FULL",
        "CARGO_FEATURE_TLS_INSECURE",
        "CARGO_FEATURE_QEMU_DEV_HTTPS",
        "CARGO_FEATURE_RAW_RELAY",
        "CARGO_FEATURE_K1_FALLBACK",
        "CARGO_FEATURE_TLS_CA_PRIVATE",
        "CARGO_FEATURE_TLS_CA_AMAZON",
        "CARGO_FEATURE_TLS_CA_LETSENCRYPT",
        "CARGO_FEATURE_TLS_CA_RSA",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
}

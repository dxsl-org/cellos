use std::fs;
use std::path::{Path, PathBuf};

// Same audited FIPS 180-4 code the kernel uses for ELF measurement.
#[path = "src/sha256.rs"]
#[allow(dead_code)]
mod sha256;

#[path = "src/posture.rs"]
#[allow(dead_code)]
mod posture;

// The syscall table parser shared with `libs/api/build.rs` (Phase 08).
include!("../libs/api/abi/table.rs");

fn main() {
    emit_syscall_opcodes();
    // Emit the Git short SHA as the snapshot invalidation key.
    // Any git commit changes this value, causing warm-boot snapshots taken before
    // that commit to be rejected (stale snapshot → cold boot).
    emit_git_sha();
    // Choose linker script based on target architecture (and board feature).
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // The private P01 trace and the G1 CPU accounting on top of it are Cargo
    // features (`p01-trace`, `g1-cpu-accounting`); the old environment switches
    // are refused rather than silently ignored.
    for retired in ["CELLOS_P01_TRACE", "CELLOS_G1_CPU"] {
        println!("cargo:rerun-if-env-changed={retired}");
        if std::env::var_os(retired).is_some_and(|value| value != "0") {
            panic!("{retired} is retired: build with the p01-trace / g1-cpu-accounting features");
        }
    }
    println!("cargo:rerun-if-env-changed=CELLOS_P01_FAULT");
    let p01 = std::env::var_os("CARGO_FEATURE_P01_TRACE").is_some();
    if p01 && target_arch != "x86_64" {
        panic!("feature p01-trace requires x86_64");
    }
    let fault = match std::env::var("CELLOS_P01_FAULT") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => String::new(),
        Err(_) => panic!("CELLOS_P01_FAULT must be valid UTF-8"),
    };
    if !matches!(
        fault.as_str(),
        "" | "contention" | "full" | "unready" | "late"
    ) || (!p01 && !fault.is_empty())
    {
        panic!("CELLOS_P01_FAULT requires private trace and a supported observer fault");
    }
    if p01 {
        println!("cargo:rustc-env=CELLOS_P01_FAULT={fault}");
    }
    if std::env::var_os("CARGO_FEATURE_G1_CPU_ACCOUNTING").is_some()
        && !(fault.is_empty() && target_os == "none")
    {
        panic!(
            "feature g1-cpu-accounting requires the x86_64 kernel trace without observer faults"
        );
    }
    let board_rpi3 = std::env::var("CARGO_FEATURE_BOARD_RPI3").is_ok();
    let board_rpi4 = std::env::var("CARGO_FEATURE_BOARD_RPI4").is_ok();
    let (ld_script, rerun_path) = match target_arch.as_str() {
        // board-rpi3/rpi4: VideoCore (and QEMU raspi3b/raspi4b) loads kernel8.img
        // at 0x80000; use the dedicated linker script.
        "aarch64" if board_rpi3 || board_rpi4 => ("kernel/linker-rpi3.ld", "kernel/linker-rpi3.ld"),
        "aarch64" => ("kernel/linker-aarch64.ld", "kernel/linker-aarch64.ld"),
        "x86_64" => ("kernel/linker-x86-64.ld", "kernel/linker-x86-64.ld"),
        "riscv32" => ("kernel/linker-riscv32.ld", "kernel/linker-riscv32.ld"),
        "arm" => ("kernel/linker-aarch32.ld", "kernel/linker-aarch32.ld"),
        "x86" => ("kernel/linker-x86-32.ld", "kernel/linker-x86-32.ld"),
        _ => ("kernel/linker.ld", "kernel/linker.ld"),
    };
    if target_os == "none" {
        println!("cargo:rustc-link-arg=-T{ld_script}");
    }
    println!("cargo:rerun-if-changed={rerun_path}");
    println!("cargo:rerun-if-changed=kernel/linker-rpi3.ld");
    println!("cargo:rerun-if-changed=kernel/linker-x86-64.ld");

    // PIE: only riscv64 (Limine KASLR). riscv32 is non-PIE (direct -kernel boot,
    // OpenSBI loads kernel at ORIGIN=0x80200000 with no relocation).
    if target_arch == "riscv64" {
        // Removed -pie and --no-dynamic-linker because they break rust-lld with static libcore.
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-env-changed=CELLOS_PHASE02_CELL_PUBLIC_KEY");
    let phase02_profile = std::env::var_os("CARGO_FEATURE_PHASE02_TERMINAL_PROFILE").is_some();
    if phase02_profile {
        assert_eq!(target_arch, "x86_64", "Phase02 fixture requires x86_64");
        assert!(
            std::env::var_os("CARGO_FEATURE_SIGNING_REQUIRED").is_some(),
            "Phase02 fixture requires signing-required"
        );
        let key_path = std::env::var_os("CELLOS_PHASE02_CELL_PUBLIC_KEY")
            .map(PathBuf::from)
            .expect("Phase02 fixture requires a private public-key file");
        println!("cargo:rerun-if-changed={}", key_path.display());
        let key = fs::read(&key_path).expect("read Phase02 public key");
        assert_eq!(key.len(), 32, "Phase02 public key must be 32 raw bytes");
        assert!(
            key.iter().any(|byte| *byte != 0),
            "zero Phase02 key refused"
        );
        fs::write(out_dir.join("phase02-cell-signing.pub"), key)
            .expect("copy private Phase02 public key");
    } else {
        assert!(
            std::env::var_os("CELLOS_PHASE02_CELL_PUBLIC_KEY").is_none(),
            "private Phase02 key requires phase02-terminal-profile"
        );
    }
    // Release posture: the cell trust anchor is a provisioned public key file
    // (the private half is a CI secret). Never together with the dev key.
    println!("cargo:rerun-if-env-changed=CELLOS_RELEASE_CELL_PUBLIC_KEY");
    let release_key = std::env::var_os("CARGO_FEATURE_RELEASE_SIGNING_KEY").is_some();
    if release_key {
        assert!(
            std::env::var_os("CARGO_FEATURE_DEV_SIGNING_KEY").is_none(),
            "release-signing-key cannot be combined with dev-signing-key \
             (build with --no-default-features --features release-posture)"
        );
        assert!(
            !phase02_profile,
            "release-signing-key cannot be combined with phase02-terminal-profile"
        );
        let key_path = std::env::var_os("CELLOS_RELEASE_CELL_PUBLIC_KEY")
            .map(PathBuf::from)
            .expect("release-signing-key requires CELLOS_RELEASE_CELL_PUBLIC_KEY (32 raw bytes)");
        println!("cargo:rerun-if-changed={}", key_path.display());
        let key = fs::read(&key_path).expect("read release cell public key");
        assert_eq!(
            key.len(),
            32,
            "release cell public key must be 32 raw bytes"
        );
        assert!(
            key.iter().any(|byte| *byte != 0),
            "zero release key refused"
        );
        // Dev key from scripts/sign-cell.py seed 0x43*32 (see kernel/src/signing.rs).
        const DEV_CELL_SIGNER_PUBKEY: [u8; 32] = [
            0x22, 0xfc, 0x29, 0x77, 0x92, 0xf0, 0xb6, 0xff, 0xc0, 0xbf, 0xcf, 0xdb, 0x7e, 0xdb,
            0x0c, 0x0a, 0xa1, 0x4e, 0x02, 0x5a, 0x36, 0x5e, 0xc0, 0xe3, 0x42, 0xe8, 0x6e, 0x38,
            0x29, 0xcb, 0x74, 0xb6,
        ];
        assert_ne!(
            key, DEV_CELL_SIGNER_PUBKEY,
            "the dev key cannot be the release key"
        );
        fs::write(out_dir.join("release-cell-signing.pub"), key).expect("copy release public key");
        // The other two trust anchors are provisioned the same way; a release
        // kernel never compiles a zero placeholder or a dev key for them.
        provision_release_anchor(
            &out_dir,
            "CELLOS_RELEASE_POLICY_PUBLIC_KEY",
            "release-policy.pub",
            &DEV_FLEET_POLICY_PUBKEY,
        );
        provision_release_anchor(
            &out_dir,
            "CELLOS_RELEASE_OWNER_PUBLIC_KEY",
            "release-owner.pub",
            &DEV_OWNER_PUBKEY,
        );
    } else {
        for var in [
            "CELLOS_RELEASE_CELL_PUBLIC_KEY",
            "CELLOS_RELEASE_POLICY_PUBLIC_KEY",
            "CELLOS_RELEASE_OWNER_PUBLIC_KEY",
        ] {
            assert!(
                std::env::var_os(var).is_none(),
                "{var} requires the release-signing-key feature"
            );
        }
    }
    let posture = derive_posture(&target_arch);
    println!("cargo:rustc-check-cfg=cfg(cellos_posture, values(\"dev\", \"test\", \"release\"))");
    println!("cargo:rustc-cfg=cellos_posture=\"{posture}\"");
    let embedded_out = out_dir.join("embedded");
    if embedded_out.exists() {
        fs::remove_dir_all(&embedded_out).expect("clear stale embedded OUT_DIR");
    }
    fs::create_dir_all(&embedded_out).expect("create embedded OUT_DIR");

    // Use arch-specific embedded directory when available, fall back to default.
    // EMBEDDED_OVERRIDE lets CI test-hooks builds point at a different directory
    // (e.g. src/embedded-test-hooks/) without touching committed source files.
    let arch_embedded = PathBuf::from(format!("src/embedded-{}", target_arch));
    println!("cargo:rerun-if-env-changed=EMBEDDED_OVERRIDE");
    // An empty value means "not set" (CI matrices pass '' for rows without one).
    let embed_override = std::env::var("EMBEDDED_OVERRIDE")
        .ok()
        .filter(|v| !v.is_empty());
    let compile_only = std::env::var_os("CARGO_FEATURE_COMPILE_ONLY").is_some();
    if compile_only && embed_override.is_none() && !p01 && !phase02_profile {
        // Build-only kernel (clippy, host tests, cargo check): it needs no image,
        // so it reads nothing from the tree. Empty `init` and VIFS1 stubs make
        // the include_bytes! sites resolve; such a kernel cannot boot.
        for stub in ["init", "kernel_fs.img"] {
            fs::write(embedded_out.join(stub), []).expect("write compile-only stub");
        }
        println!(
            "cargo:rustc-env=EMBEDDED_OUT_DIR={}",
            embedded_out.display()
        );
        return;
    }
    let embedded_src = if let Some(ov) = embed_override.clone() {
        // Build scripts run with CWD = kernel/, but callers (CI scripts,
        // run.ps1) usually pass a workspace-root-relative path like
        // "kernel/src/embedded-test-hooks" — try both. A misresolved override
        // must FAIL the build: the old silent fallback shipped kernels with an
        // empty VIFS1 stub that booted to "bootstrap not in VIFS1".
        let p = PathBuf::from(&ov);
        let from_workspace = PathBuf::from("..").join(&p);
        if p.exists() {
            p
        } else if from_workspace.exists() {
            from_workspace
        } else {
            panic!(
                "EMBEDDED_OVERRIDE={ov} does not exist (checked relative to \
                 kernel/ and the workspace root)"
            );
        }
    } else {
        // Fallback chain (deprecated, architecture remediation Phase 03): the
        // kernel then embeds whatever image is lying in the tree, so the binary is
        // not a function of the build inputs. Callers must pass EMBEDDED_OVERRIDE;
        // this becomes a hard error once every lane does.
        let fallback = if board_rpi3 && PathBuf::from("../target/rpi3-embedded").exists() {
            PathBuf::from("../target/rpi3-embedded")
        } else if arch_embedded.exists() {
            arch_embedded
        } else {
            PathBuf::from("src/embedded")
        };
        println!(
            "cargo:warning=EMBEDDED_OVERRIDE not set: embedding the in-tree fallback {} \
             (deprecated; pass EMBEDDED_OVERRIDE=<dir with init + kernel_fs.img>, or \
             --features compile-only for a build that never boots)",
            fallback.display()
        );
        fallback
    };
    verify_embed_manifest(&embedded_src);
    if phase02_profile {
        assert!(
            embed_override.is_some(),
            "Phase02 requires an explicit private embedded directory"
        );
        let private =
            fs::canonicalize(&embedded_src).expect("resolve private Phase02 embedded directory");
        let canonical =
            fs::canonicalize("src/embedded-x86_64").expect("resolve canonical embedded directory");
        assert_ne!(private, canonical, "canonical Phase02 embedding refused");
        for artifact in ["init", "kernel_fs.img"] {
            let source =
                fs::canonicalize(private.join(artifact)).expect("Phase02 embedded input missing");
            assert!(
                !source.starts_with(&canonical),
                "canonical Phase02 embedded input refused"
            );
            assert!(
                fs::metadata(&source).expect("stat Phase02 input").len() != 0,
                "empty Phase02 embedded input refused"
            );
        }
    }
    if p01 {
        let src = embedded_src.join("p01_manifest.bin");
        println!("cargo:rerun-if-changed={}", src.display());
        let manifest = fs::read(&src).expect("private P01 build requires p01_manifest.bin");
        assert_eq!(
            manifest.len(),
            96,
            "P01 manifest is exactly three SHA256 digests"
        );
        fs::write(embedded_out.join("p01_manifest.bin"), manifest).expect("embed P01 manifest");
    }
    // Only two artifacts are actually embedded: `init` (kernel/src/main.rs
    // INIT_ELF) and `kernel_fs.img` (ramdisk.rs VIFS1 — bootstrap cells only,
    // G2 kernel-shrink). Everything else ships in the disk cell-store.
    let cells = ["init", "kernel_fs.img"];

    for cell in &cells {
        let src = embedded_src.join(cell);
        if !src.exists() {
            // kernel_fs.img may be pre-committed for canonical targets
            // (e.g. embedded-x86_64, signed and tracked via manifest.json)
            // or dynamically assembled by gen_disk.ps1 / build-*-ci.sh via
            // EMBEDDED_OVERRIDE. For compile-only contexts where it is absent,
            // emit an empty stub so include_bytes! in ramdisk.rs resolves —
            // a kernel built from the stub compiles but has no VIFS1.
            if *cell == "kernel_fs.img" {
                fs::write(embedded_out.join(cell), []).expect("write kernel_fs.img stub");
                if std::env::var_os("CARGO_FEATURE_COMPILE_ONLY").is_none() {
                    println!(
                        "cargo:warning={} missing — embedded an EMPTY VIFS1 stub. \
                         This kernel is compile-only; assemble the real image \
                         (scripts/assemble-x86-image.py / scripts/gen-disk*-ci.sh) or \
                         build with --features compile-only. This becomes an error.",
                        src.display()
                    );
                }
            }
            continue;
        }
        let dst = embedded_out.join(cell);
        println!("cargo:rerun-if-changed={}", src.display());

        fs::copy(&src, &dst).expect("copy embedded cell");

        // `init` is already signed by the image assembly pipeline. Its signature
        // covers the final ELF container except the signature payload itself, so
        // any post-sign strip or section-table rewrite would invalidate admission.
    }

    // Expose the copied embedded directory to source via env! macro.
    println!(
        "cargo:rustc-env=EMBEDDED_OUT_DIR={}",
        embedded_out.display()
    );
}

/// Refuse to embed `init` / `kernel_fs.img` whose bytes differ from the
/// directory's `manifest.json` (written by `scripts/assemble-x86-image.py`,
/// schema `cellos-x86-image-manifest/1`). Directories without a manifest
/// (RV64/AArch64 recipes, compile-only stubs) are not checked yet.
///
/// Panics with the mismatching artifact named, so a stale or swapped image
/// cannot be embedded into a kernel that claims the manifest's provenance.
fn verify_embed_manifest(dir: &Path) {
    let manifest_path = dir.join("manifest.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let Ok(manifest) = fs::read_to_string(&manifest_path) else {
        return;
    };
    if !manifest.contains("\"cellos-x86-image-manifest/1\"") {
        return;
    }
    for (key, file) in [("kernel_fs_img", "kernel_fs.img"), ("init", "init")] {
        let expected = manifest_digest(&manifest, key)
            .unwrap_or_else(|| panic!("{}: no \"{key}\".sha256 entry", manifest_path.display()));
        let path = dir.join(file);
        let bytes = fs::read(&path).unwrap_or_else(|e| {
            panic!("{} listed in manifest but unreadable: {e}", path.display())
        });
        let actual: String = sha256::sha256(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            actual,
            expected,
            "{} does not match {} (stale or swapped embed input; re-run the image assembler)",
            path.display(),
            manifest_path.display()
        );
    }
}

/// The `sha256` string of the top-level object `"<key>": { … "sha256": "<hex>" … }`.
/// Top-level keys of this schema appear once and precede their own digest.
fn manifest_digest(manifest: &str, key: &str) -> Option<String> {
    let start = manifest.find(&format!("\n  \"{key}\": {{"))?;
    let object = &manifest[start..];
    let object = &object[..object.find("\n  }")?];
    let digest = object.split("\"sha256\": \"").nth(1)?;
    let digest = &digest[..digest.find('"')?];
    (digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())).then(|| digest.to_owned())
}

/// Dev policy root (`scripts/sign-policy.py` fixed seed); see `kernel/src/policy.rs`.
const DEV_FLEET_POLICY_PUBKEY: [u8; 32] = [
    0x21, 0x52, 0xf8, 0xd1, 0x9b, 0x79, 0x1d, 0x24, 0x45, 0x32, 0x42, 0xe1, 0x5f, 0x2e, 0xab, 0x6c,
    0xb7, 0xcf, 0xfa, 0x7b, 0x6a, 0x5e, 0xd3, 0x00, 0x97, 0x96, 0x0e, 0x06, 0x98, 0x81, 0xdb, 0x12,
];

/// Dev owner admission key (seed `[0x4F; 32]`); see `kernel/src/admission/owner_key.rs`.
const DEV_OWNER_PUBKEY: [u8; 32] = [
    0x00, 0xe3, 0xc5, 0x6b, 0x91, 0xab, 0x0a, 0x01, 0x71, 0x74, 0xb9, 0x66, 0x45, 0xea, 0xf9, 0x28,
    0x36, 0x6c, 0xdb, 0xae, 0x1e, 0x87, 0xfd, 0x21, 0xbf, 0x86, 0x66, 0x1d, 0x86, 0xf3, 0xe7, 0xef,
];

/// Copy the 32-byte public key named by `var` into `OUT_DIR/out_name`.
///
/// Panics when the variable is missing, the key is not 32 bytes, all zero, or
/// equal to the matching dev key: a release kernel must not trust a placeholder
/// or a key whose private half is in the repository.
fn provision_release_anchor(out_dir: &Path, var: &str, out_name: &str, dev_key: &[u8; 32]) {
    println!("cargo:rerun-if-env-changed={var}");
    let path = std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("release-signing-key requires {var} (32 raw bytes)"));
    println!("cargo:rerun-if-changed={}", path.display());
    let key = fs::read(&path).unwrap_or_else(|e| panic!("read {var} ({}): {e}", path.display()));
    assert_eq!(key.len(), 32, "{var} must name a 32-byte raw public key");
    assert!(key.iter().any(|b| *b != 0), "{var}: zero key refused");
    assert_ne!(
        key.as_slice(),
        dev_key,
        "{var}: the dev key cannot be a release anchor"
    );
    fs::write(out_dir.join(out_name), key).expect("copy release anchor");
}

/// Enforce the single trust posture (`kernel/src/posture.rs`) and return its name.
fn derive_posture(target_arch: &str) -> &'static str {
    let on = |feature: &str| {
        let var = format!("CARGO_FEATURE_{}", feature.to_uppercase().replace('-', "_"));
        std::env::var_os(var).is_some()
    };
    match posture::derive(target_arch, &on) {
        Ok(posture) => posture.as_str(),
        Err(reason) => panic!("kernel feature set refused: {reason}"),
    }
}

/// Emit the git short SHA via cargo:rustc-env for snapshot invalidation.
/// Falls back to a placeholder ("00000000") when not in a git repository.
fn emit_git_sha() {
    // Use vergen-gitcl to read the git SHA; ignore errors gracefully.
    let git = vergen_gitcl::GitclBuilder::default().sha(true).build().ok();
    let mut emitter = vergen_gitcl::Emitter::default();
    if let Some(g) = git {
        let _ = emitter.add_instructions(&g);
    }
    if emitter.emit().is_err() || std::env::var("VERGEN_GIT_SHA").is_err() {
        // Not a git repo or vergen failed — emit a stable placeholder.
        // Any non-zero placeholder is fine; it will match itself on rebuild.
        println!("cargo:rustc-env=VERGEN_GIT_SHA=00000000");
    }
}

/// Generate `syscall_opcodes.rs` from `libs/api/abi/syscalls.toml`: the opcode
/// each decoded kernel `Syscall` variant came from (`opcode_of`) and the raw
/// opcodes outside `ViSyscall` with their gate bit (`RAW_OPCODES`). The match is
/// exhaustive, so a kernel variant missing from the table fails the build.
fn emit_syscall_opcodes() {
    use std::fmt::Write as _;
    let table = Path::new(env!("CARGO_MANIFEST_DIR")).join("../libs/api/abi/syscalls.toml");
    println!("cargo:rerun-if-changed={}", table.display());
    println!("cargo:rerun-if-changed=../libs/api/abi/table.rs");
    let text = fs::read_to_string(&table).expect("read libs/api/abi/syscalls.toml");
    let (rows, raws, internal) = parse_table(&text);
    let mut out = String::from(
        "// @generated by kernel/build.rs from libs/api/abi/syscalls.toml — do not edit.\n\n\
         /// Opcode a decoded [`Syscall`] corresponds to, for callers that hold a\n\
         /// decoded value rather than a trap opcode. A variant shared by an api row\n\
         /// and a raw alias maps to the api row (the gated one); a variant with no\n\
         /// opcode maps to `usize::MAX`, which the gate treats as unknown.\n\
         // Generated: one uniform `{ .. }` pattern whatever the variant shape.\n\
         #[allow(clippy::unneeded_struct_pattern)]\n\
         pub(super) fn opcode_of(syscall: &Syscall) -> usize {\n    match syscall {\n",
    );
    let mut seen = std::collections::BTreeSet::new();
    for row in rows.iter().filter(|r| r.name != "Unknown") {
        let kernel = row.kernel.clone().unwrap_or_else(|| row.name.clone());
        if seen.insert(kernel.clone()) {
            writeln!(
                out,
                "        Syscall::{kernel} {{ .. }} => {},",
                row.number.expect("number")
            )
            .unwrap();
        }
    }
    for raw in &raws {
        if seen.insert(raw.kernel.clone()) {
            if let Some(feature) = &raw.feature {
                writeln!(out, "        #[cfg(feature = \"{feature}\")]").unwrap();
            }
            writeln!(
                out,
                "        Syscall::{} {{ .. }} => {},",
                raw.kernel,
                raw.id.expect("id")
            )
            .unwrap();
        }
    }
    for kernel in &internal {
        assert!(seen.insert(kernel.clone()), "{kernel} listed twice");
        writeln!(out, "        Syscall::{kernel} {{ .. }} => usize::MAX,").unwrap();
    }
    out.push_str("    }\n}\n\n");
    out.push_str(
        "/// Raw opcodes outside `ViSyscall` that the trap decodes, with the allowlist\n\
         /// bit that gates each (`None` = always permitted). Feature-gated raw\n\
         /// opcodes are handled where their feature is.\n\
         pub(super) const RAW_OPCODES: &[(usize, Option<u8>)] = &[\n",
    );
    for raw in raws.iter().filter(|r| r.feature.is_none()) {
        let bit = raw
            .bit
            .map_or(String::from("None"), |b| format!("Some({b})"));
        writeln!(out, "    ({}, {bit}),", raw.id.expect("id")).unwrap();
    }
    out.push_str("];\n\n");
    for raw in raws.iter().filter(|r| r.gate_as.is_some()) {
        let feature = raw
            .feature
            .as_deref()
            .expect("a gate_as row is feature-gated");
        writeln!(
            out,
            "/// Raw opcode {id} is admitted exactly as `{g}` (bit and rights).\n\
             #[cfg(feature = \"{feature}\")]\n\
             pub(super) const RAW_GATE_AS: (usize, api::syscall::ViSyscall) =\n    ({id}, api::syscall::ViSyscall::{g});",
            id = raw.id.expect("id"),
            g = raw.gate_as.as_ref().expect("gate_as"),
        )
        .unwrap();
    }
    let path = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("syscall_opcodes.rs");
    fs::write(path, out).expect("write syscall_opcodes.rs");
}

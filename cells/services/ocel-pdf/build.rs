// SPDX-License-Identifier: AGPL-3.0-or-later
use std::{env, fs, path::{Path, PathBuf}};

fn main() {
    let target = env::var("TARGET").expect("Cargo TARGET");
    assert_eq!(target, "riscv64gc-unknown-none-elf",
        "ocel-pdf requires riscv64gc-unknown-none-elf and an RV64 lp64d C compiler; no unsupported-target stub exists");
    for path in ["vendor", "src/c"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let vendor = Path::new("vendor/mupdf");
    // Use the pinned package's own curated dependency lists, not host libraries.
    let lists = fs::read_to_string(vendor.join("Makelists")).expect("pinned MuPDF Makelists");
    let mut b = cc::Build::new();
    env::remove_var(format!("CFLAGS_{}", target.replace('-', "_")));
    if env::var("CC_riscv64gc_unknown_none_elf").is_err() {
        b.compiler("riscv-none-elf-gcc");
    }
    b.include("vendor/include").include("vendor").include(vendor.join("include"));
    for path in ["thirdparty/freetype/include", "scripts/freetype", "thirdparty/libjpeg",
        "scripts/libjpeg", "thirdparty/zlib", "thirdparty/jbig2dec",
        "thirdparty/openjpeg/src/lib/openjp2", "thirdparty/lcms2/include"] {
        b.include(vendor.join(path));
    }
    b.flag("-include").flag("cellos-config.h")
        .flag("-std=gnu11").flag("-ffreestanding").flag("-fno-stack-protector")
        .flag("-fno-unwind-tables").flag("-fno-asynchronous-unwind-tables")
        .flag("-ffunction-sections").flag("-fdata-sections")
        .flag("-march=rv64gc").flag("-mabi=lp64d").flag("-mcmodel=medany")
        .flag("-Werror=implicit-function-declaration").opt_level_str("s").pic(true).warnings(false);
    let omit = ["harfbuzz.c", "memento.c", "time.c", "directory.c", "document-all.c",
        "output-docx.c", "output-pdfocr.c", "ocr-device.c", "leptonica-wrap.c"];
    for dir in ["source/fitz", "source/pdf"] {
        let mut files: Vec<PathBuf> = fs::read_dir(vendor.join(dir)).unwrap()
            .map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "c"))
            .filter(|p| !omit.contains(&p.file_name().unwrap().to_str().unwrap())).collect();
        files.sort();
        b.files(files);
    }
    for line in lists.lines() {
        if ["FREETYPE_SRC", "LIBJPEG_SRC", "ZLIB_SRC", "JBIG2DEC_SRC", "OPENJPEG_SRC", "LCMS2_SRC"]
            .iter().any(|key| line.starts_with(&format!("{key} += "))) {
            b.file(vendor.join(line.split(" += ").nth(1).unwrap().trim()));
        }
    }
    // Embed standard PDF base14 fonts only. CJK uses document-embedded fonts:
    // shipping Droid fallback exceeds the existing VIFS1 loader's heap.
    // Generate exact upstream noto.c names without host objcopy tooling.
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut fonts: Vec<PathBuf> = fs::read_dir(vendor.join("resources/fonts/urw")).unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "cff"))
        .collect();
    fonts.sort();
    for (i, font) in fonts.iter().enumerate() {
        let data = fs::read(font).unwrap();
        let symbol = font.file_name().unwrap().to_str().unwrap().replace(['.', '-'], "_");
        let mut source = format!("const unsigned int _binary_{symbol}_size = {};\nconst unsigned char _binary_{symbol}[] = {{\n", data.len());
        for bytes in data.chunks(32) {
            for byte in bytes { source.push_str(&format!("{byte},")); }
            source.push('\n');
        }
        source.push_str("};\n");
        let generated = out.join(format!("font-{i}.c"));
        fs::write(&generated, source).unwrap();
        b.file(generated);
    }
    b.file("src/c/renderer.c").file("src/c/libc.c").file("src/c/setjmp.S");
    b.compile("ocel_pdf_mupdf");
    println!("cargo:rustc-link-arg=--strip-all");
    cell_build::emit_linker_script();
}

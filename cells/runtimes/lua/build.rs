fn main() {
    let mut build = cc::Build::new();

    let lua_src = ["lapi.c", "lcode.c", "lctype.c", "ldebug.c", "ldo.c", "ldump.c", "lfunc.c", "lgc.c", "llex.c", "lmem.c", "lobject.c", "lopcodes.c", "lparser.c", "lstate.c", "lstring.c", "ltable.c", "ltm.c", "lundump.c", "lvm.c", "lzio.c", "lauxlib.c", "lbaselib.c", "lcorolib.c", "ldblib.c", "liolib.c", "lmathlib.c", "loadlib.c", "loslib.c", "lstrlib.c", "ltablib.c", "lutf8lib.c", "linit.c"];

    for file in &lua_src {
        build.file(format!("src/c/src/{}", file));
    }
    build.file("src/c/src/vi_shim.c");

    build
        .compiler("riscv-none-elf-gcc")
        .archiver("riscv-none-elf-ar")
        .flag("-mabi=lp64d")
        .include("src/c/src")
        .define("LUA_USE_C99", None)
        .compile("lua");
    
    println!("cargo:rustc-link-search=native=C:/RISCV/xpack-riscv-none-elf-gcc-15.2.0-1/riscv-none-elf/lib/rv64imafdc_zicsr_zaamo_zalrsc/lp64d");
    println!("cargo:rustc-link-lib=static=c");
    println!("cargo:rustc-link-lib=static=m");
    // rust-lld requires flag directly
    println!("cargo:rustc-link-arg=--allow-multiple-definition");
    println!("cargo:rerun-if-changed=src/c/src");
}

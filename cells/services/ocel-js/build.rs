fn main() {
    // Without this the cell links at the linker's default VA (0x10000) instead of
    // as a `-pie` at VA 0, and the kernel refuses it at spawn with
    // "load VA 0x101000 already mapped" (collision with the kernel image).
    cell_build::emit_linker_script();
}

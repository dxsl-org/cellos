fn main() {
    // Compile all .vi files under src/ui/ → $OUT_DIR/vi_generated/*.rs
    vi_build::compile_vi_dir("src/ui/");
}

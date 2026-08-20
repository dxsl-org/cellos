fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") {
        cell_build::emit_linker_script();
    }
}

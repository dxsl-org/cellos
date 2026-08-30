fn main() {
    if let Err(error) = vf2_root_stream_bundler::bundler::run(std::env::args_os()) {
        eprintln!("vf2-root-stream-bundler: {error}");
        std::process::exit(2);
    }
}

fn main() {
    match vf2_root_stream_bundler::verifier::run(std::env::args_os()) {
        Ok(report) => print!("{report}"),
        Err(error) => {
            eprintln!("vf2-root-stream-verifier: {error}");
            std::process::exit(2);
        }
    }
}

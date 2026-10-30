//! Stack budget (K5 CP1 section 11): the full wrapped sign on a thread with a
//! cell-sized stack. A pass is a host proof that the unlock call chain fits;
//! it is not a measurement on Cellos.

use std::boxed::Box;

use super::fixtures::*;
use super::*;

/// 64 KiB when optimized (the shipped cell is built `--release` and has a
/// 256 KiB stack, so this keeps a 4x margin); an unoptimized test build has
/// larger frames, so it gets the cell's full 256 KiB.
const STACK_BYTES: usize = if cfg!(debug_assertions) {
    256 * 1024
} else {
    64 * 1024
};

#[test]
fn wrapped_unlock_and_sign_fit_the_stack_budget() {
    let handle = std::thread::Builder::new()
        .name("kms-stack-budget".into())
        .stack_size(STACK_BYTES)
        .spawn(|| {
            // The service lives on the heap: only the call chain under test
            // is charged to the small stack.
            let mut service = Box::new(service());
            let admin = Actor::admin();
            let login = Actor::login();
            let blob = Craft::wrapped(&RFC8032_SEED_1, &[b'p'; 1024], &[3u8; 64], 1).bytes();
            import_blob(&mut *service, &admin, b"wk", &blob).unwrap();
            let public = public_of(&RFC8032_SEED_1);
            stage_secret(&mut *service, &login, public, &[b'p'; 1024]).unwrap();
            sign(&mut *service, &login, public, SESSION_ID, &[b'u'; 31]).unwrap();
        })
        .unwrap();
    handle.join().expect("no stack overflow, no panic");
}

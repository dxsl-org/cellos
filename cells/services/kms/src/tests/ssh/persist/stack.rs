//! Stack budget: the persister frames (load, store, tombstone) on a thread
//! with a cell-sized stack, as the K5 stack test. A pass is a host proof that
//! the call chain fits; it is not a measurement on Cellos.

use std::boxed::Box;

use super::*;

/// 64 KiB when optimized (the shipped cell is built `--release` and has a
/// 256 KiB stack); the unoptimized test build has larger frames, so it gets
/// the cell's full 256 KiB.
const STACK_BYTES: usize = if cfg!(debug_assertions) {
    256 * 1024
} else {
    64 * 1024
};

#[test]
fn persister_load_store_and_tombstone_fit_the_stack_budget() {
    let handle = std::thread::Builder::new()
        .name("kms-persist-stack".into())
        .stack_size(STACK_BYTES)
        .spawn(|| {
            let admin = Actor::admin();
            // Largest wrapped record: a 2,032-byte section and a 64-byte
            // salt, so the record is as large as the store budget allows.
            let mut craft = Craft::wrapped(&RFC8032_SEED_1, b"p", &[3u8; 64], 1);
            let inner = inner_section(&RFC8032_SEED_1, &[b'c'; 1_900], 16, Pad::Natural);
            craft.private = wrap_inner(&inner, b"p", &[3u8; 64], 1);
            let blob = craft.bytes();
            // The leaf lives on the heap: only the call chain under test is
            // charged to the small stack.

            let mut leaf = Box::new(fresh());
            import_blob(&mut *leaf, &admin, NAME, &blob).unwrap(); // load + store
            let mut again = Box::new(reboot(&leaf));
            status(&mut *again, &Actor::admin(), NAME).unwrap(); // load, found
            delete_key(&mut *again, &admin, NAME, public_of(&RFC8032_SEED_1)).unwrap();
            // tombstone
        })
        .unwrap();
    handle.join().expect("no stack overflow, no panic");
}

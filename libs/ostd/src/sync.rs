/// Synchronisation primitives for cell code.
///
/// Re-exports `spin::Mutex` as the canonical lock type. Using this
/// instead of `RefCell` gives correct `Sync` bounds without `unsafe`.
pub use spin::Mutex;
pub use spin::MutexGuard;

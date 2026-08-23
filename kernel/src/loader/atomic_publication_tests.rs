//! Deterministic boot-time failure injection for atomic ELF publication.

mod baseline;
mod cases;
mod denials;
mod harness;
mod probe;
mod snapshot;
mod success;

pub(crate) use harness::{checkpoint, observe_complete};

pub(crate) fn observe_schedule_attempt() {
    harness::observe_schedule_attempt();
}

pub(crate) fn observe_unpublished_segments(segments: &crate::task::stack::CellSegments) {
    probe::observe_unpublished(segments);
}

pub(crate) fn observe_unpublished_pages(pages: &[crate::loader::elf::LoadedPage]) {
    probe::observe_unpublished_pages(pages);
}
/// Exercise the normal governed in-memory API without relying on a boot-image
/// cell that the test-hooks image does not embed. `INIT_ELF` is a real embedded
/// ELF with its ordinary manifest; the `/bin/platform` path supplies the normal
/// path capability request, ceiling, and policy lookup.
///
/// The test-hooks image's embedded init is signed by its normal image build;
/// this helper neither synthesizes a signature nor bypasses the signature gate.
pub(super) fn spawn_governed_platform(request: super::SpawnRequest) -> types::ViResult<usize> {
    super::spawn_gated(crate::INIT_ELF, "/bin/platform", request)
}

/// Publish the signed, authority-free probe packaged only in the test-hooks
/// VIFS1 image. This reaches the ordinary file lookup, signature, policy, and
/// governed publication path before normal boot.
#[cfg(target_arch = "riscv64")]
pub(super) fn spawn_governed_probe() -> types::ViResult<usize> {
    super::spawn_from_path("/bin/atomic-probe", super::SpawnRequest::governed_boot())
}

pub(crate) fn begin_governed_attempt() {
    success::begin_governed_attempt();
}

pub(crate) fn finish_governed_attempt(result: &types::ViResult<usize>) {
    match result {
        Ok(tid) if success::governed_attempt_pending() => {
            cases::finish_governed_success_case(*tid);
        }
        Ok(_) => {}
        Err(_) => success::abort_governed_attempt(),
    }
}

pub(crate) fn arm_trusted_success() {
    cases::arm_trusted_success_case();
}

#[cfg(target_arch = "riscv64")]
pub(crate) fn run_governed_success_after_secondaries() {
    cases::run_governed_success_after_secondaries();
}

pub(crate) fn finish_trusted_success(tid: usize) {
    cases::finish_trusted_success_case(tid);
}

pub(super) fn run_all() {
    cases::run_all();
}

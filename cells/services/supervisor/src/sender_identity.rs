//! Who may ask the supervisor for power and hotswap: the kernel-attested
//! sender Cell's measured program (architecture remediation Phase 06, CP1 A).

/// Pure rule behind `sender_is_program` (main.rs): an attested trailer for this exact
/// sender, and a measured image for that `(cell, generation)` signed as
/// `program`. A missing trailer or record is an unknown caller: refused.
pub(crate) fn caller_runs_program(
    caller: Option<api::caller_identity::CallerIdentity>,
    sender_tid: usize,
    program: &str,
    resolve: impl Fn(u64, u64) -> Option<api::cell_image::CellImage>,
) -> bool {
    let Some(caller) = caller else {
        return false;
    };
    caller.sender_tid == sender_tid as u64
        && resolve(caller.cell_id, caller.generation)
            .is_some_and(|image| image.identity.is_signed_program(program))
}

#[cfg(test)]
mod tests {
    use super::caller_runs_program;
    use api::caller_identity::CallerIdentity;
    use api::cell_image::{CellImage, ImageIdentity, ANCHOR_DEV};

    const CELL: u64 = 40;
    const GENERATION: u64 = 7;
    const SENDER: usize = 41;

    fn caller(sender_tid: usize) -> Option<CallerIdentity> {
        Some(CallerIdentity {
            flags: 0,
            cell_id: CELL,
            generation: GENERATION,
            sender_tid: sender_tid as u64,
        })
    }

    fn image(identity: ImageIdentity) -> impl Fn(u64, u64) -> Option<CellImage> {
        move |cell_id, generation| {
            (cell_id == CELL && generation == GENERATION).then_some(CellImage {
                cell_id,
                generation,
                identity,
            })
        }
    }

    #[test]
    fn only_the_attested_signed_program_is_accepted() {
        let shutdown = ImageIdentity::signed([1; 32], ANCHOR_DEV, b"shutdown").expect("identity");
        let cellbox = ImageIdentity::signed([2; 32], ANCHOR_DEV, b"cellbox").expect("identity");
        let unsigned = ImageIdentity::unsigned([3; 32]);

        assert!(caller_runs_program(
            caller(SENDER),
            SENDER,
            "shutdown",
            image(shutdown)
        ));
        // Another signed program, unsigned bytes, or no record (441 refused:
        // not a reviewed install) are all refused.
        assert!(!caller_runs_program(
            caller(SENDER),
            SENDER,
            "shutdown",
            image(cellbox)
        ));
        assert!(!caller_runs_program(
            caller(SENDER),
            SENDER,
            "shutdown",
            image(unsigned)
        ));
        assert!(!caller_runs_program(
            caller(SENDER),
            SENDER,
            "shutdown",
            |_, _| None
        ));
        // No trailer, or a trailer for a different sender, is an unknown caller.
        assert!(!caller_runs_program(
            None,
            SENDER,
            "shutdown",
            image(shutdown)
        ));
        assert!(!caller_runs_program(
            caller(SENDER + 1),
            SENDER,
            "shutdown",
            image(shutdown)
        ));
    }
}

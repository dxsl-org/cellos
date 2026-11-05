//! Host tests for the Sas release lease: a retired page fails the in-window
//! re-validation, and the release side cannot return while a window that
//! validated the page is still copying.

use super::*;
use crate::task::user_copy::sv39_probe::{SV39_READ, SV39_USER, SV39_VALID, SV39_WRITE};
use alloc::sync::Arc;
use core::sync::atomic::AtomicBool;
use std::thread;
use std::time::Duration;

const USER_RW: usize = SV39_VALID | SV39_READ | SV39_WRITE | SV39_USER;
const KERNEL_RWX: usize = SV39_VALID | SV39_READ | SV39_WRITE;

#[test]
fn retired_page_fails_in_window_revalidation() {
    let live = |_page: usize| Some(USER_RW);
    assert!(sas_span_live(0x4000_0000, 3 * 4096, Direction::ToUser, live).is_ok());

    // The free path re-maps the identity VA kernel-RWX: writable from the
    // kernel, so only the USER check stops the commit.
    let remapped = |page: usize| {
        Some(if page == 0x4000_1000 {
            KERNEL_RWX
        } else {
            USER_RW
        })
    };
    assert!(sas_span_live(0x4000_0000, 3 * 4096, Direction::ToUser, remapped).is_err());
    assert!(sas_span_live(0x4000_0000, 3 * 4096, Direction::FromUser, remapped).is_err());

    let unmapped = |page: usize| (page != 0x4000_2000).then_some(USER_RW);
    assert!(sas_span_live(0x4000_0000, 3 * 4096, Direction::ToUser, unmapped).is_err());
}

#[test]
fn release_waits_for_an_open_window() {
    let seqs = Arc::new(WindowSeqs::<4>::new());
    let window = seqs.open(2).expect("hart 2 in range");
    let released = Arc::new(AtomicBool::new(false));
    let waiter = {
        let seqs = Arc::clone(&seqs);
        let released = Arc::clone(&released);
        thread::spawn(move || {
            seqs.wait_quiescent();
            released.store(true, Ordering::SeqCst);
        })
    };
    thread::sleep(Duration::from_millis(50));
    assert!(
        !released.load(Ordering::SeqCst),
        "release returned mid-copy"
    );
    drop(window);
    waiter.join().expect("waiter");
    assert!(released.load(Ordering::SeqCst));

    // Closed windows and out-of-range harts never block or open.
    seqs.wait_quiescent();
    assert!(seqs.open(4).is_none());
}

/// Model of the full protocol: the commit re-walks the PTE inside its window
/// and "writes" the frame; the release clears the PTE, waits, then frees.
/// No write may ever observe the frame already freed.
#[test]
fn no_commit_write_lands_after_release() {
    for _ in 0..200 {
        let seqs = Arc::new(WindowSeqs::<2>::new());
        let pte = Arc::new(AtomicUsize::new(USER_RW));
        let started = Arc::new(AtomicBool::new(false));
        let freed = Arc::new(AtomicBool::new(false));
        let late_write = Arc::new(AtomicBool::new(false));
        let commit = {
            let (seqs, pte, started, freed, late_write) = (
                Arc::clone(&seqs),
                Arc::clone(&pte),
                Arc::clone(&started),
                Arc::clone(&freed),
                Arc::clone(&late_write),
            );
            thread::spawn(move || loop {
                let window = seqs.open(0).expect("hart 0");
                started.store(true, Ordering::SeqCst);
                let leaf = |_page: usize| Some(pte.load(Ordering::Relaxed));
                if sas_span_live(0x8000, 16, Direction::ToUser, leaf).is_err() {
                    return;
                }
                // The byte loop: long enough that a release which skips the
                // wait frees the frame while this copy is still running.
                for _ in 0..20_000 {
                    core::hint::spin_loop();
                }
                if freed.load(Ordering::SeqCst) {
                    late_write.store(true, Ordering::SeqCst);
                }
                drop(window);
            })
        };
        while !started.load(Ordering::SeqCst) {
            core::hint::spin_loop();
        }
        pte.store(KERNEL_RWX, Ordering::Relaxed);
        seqs.wait_quiescent();
        freed.store(true, Ordering::SeqCst);
        commit.join().expect("commit");
        assert!(
            !late_write.load(Ordering::SeqCst),
            "commit wrote a freed frame"
        );
    }
}

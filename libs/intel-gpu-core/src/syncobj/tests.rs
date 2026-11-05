use super::*;

#[test]
fn mesa_init_probe_signalled_create_then_zero_timeout_wait_all() {
    // vk_drm_syncobj.c probe: create SIGNALED, WAIT(timeout 0, WAIT_ALL) must succeed.
    let mut s = Syncobjs::new();
    let h = s.create(CREATE_SIGNALED).unwrap();
    assert_eq!(
        s.poll_wait(&[h], &[], WAIT_ALL, false),
        Ok(WaitState::Ready { first: 0 })
    );
}

#[test]
fn binary_wait_on_empty_payload_needs_wait_for_submit() {
    let mut s = Syncobjs::new();
    let h = s.create(0).unwrap();
    assert_eq!(s.poll_wait(&[h], &[], 0, false), Err(SyncError::Invalid));
    assert_eq!(
        s.poll_wait(&[h], &[], WAIT_FOR_SUBMIT, false),
        Ok(WaitState::Pending)
    );
    let f = s.new_fence();
    s.attach(h, 0, f).unwrap();
    assert_eq!(
        s.poll_wait(&[h], &[], WAIT_FOR_SUBMIT, false),
        Ok(WaitState::Pending)
    );
    s.signal_fence(f);
    assert_eq!(
        s.poll_wait(&[h], &[], 0, false),
        Ok(WaitState::Ready { first: 0 })
    );
}

#[test]
fn wait_any_reports_first_signalled_index_and_wait_all_waits_for_every_one() {
    let mut s = Syncobjs::new();
    let (a, b) = (s.create(0).unwrap(), s.create(0).unwrap());
    let (fa, fb) = (s.new_fence(), s.new_fence());
    s.attach(a, 0, fa).unwrap();
    s.attach(b, 0, fb).unwrap();
    s.signal_fence(fb);
    assert_eq!(
        s.poll_wait(&[a, b], &[], 0, false),
        Ok(WaitState::Ready { first: 1 })
    );
    assert_eq!(
        s.poll_wait(&[a, b], &[], WAIT_ALL, false),
        Ok(WaitState::Pending)
    );
    s.signal_fence(fa);
    assert_eq!(
        s.poll_wait(&[a, b], &[], WAIT_ALL, false),
        Ok(WaitState::Ready { first: 0 })
    );
}

#[test]
fn timeline_points_complete_in_order() {
    let mut s = Syncobjs::new();
    let t = s.create(0).unwrap();
    let (f1, f2) = (s.new_fence(), s.new_fence());
    s.attach(t, 1, f1).unwrap();
    s.attach(t, 2, f2).unwrap();
    // Point 2's own fence finishing first does not complete point 2.
    s.signal_fence(f2);
    assert_eq!(s.poll_wait(&[t], &[2], 0, true), Ok(WaitState::Pending));
    assert_eq!(s.query(t, 0), Ok(0));
    s.signal_fence(f1);
    assert_eq!(
        s.poll_wait(&[t], &[2], 0, true),
        Ok(WaitState::Ready { first: 0 })
    );
    assert_eq!(s.query(t, 0), Ok(2));
}

#[test]
fn waiting_for_a_point_between_submitted_points_uses_the_next_one() {
    let mut s = Syncobjs::new();
    let t = s.create(0).unwrap();
    let f = s.new_fence();
    s.attach(t, 10, f).unwrap();
    assert_eq!(s.poll_wait(&[t], &[5], 0, true), Ok(WaitState::Pending));
    s.signal_fence(f);
    assert_eq!(
        s.poll_wait(&[t], &[5], 0, true),
        Ok(WaitState::Ready { first: 0 })
    );
}

#[test]
fn unsubmitted_point_is_invalid_unless_waiting_for_submit_or_availability() {
    // vk_queue.c: zero-timeout TIMELINE_WAIT|WAIT_AVAILABLE before every submit.
    let mut s = Syncobjs::new();
    let t = s.create(0).unwrap();
    assert_eq!(s.poll_wait(&[t], &[3], 0, true), Err(SyncError::Invalid));
    assert_eq!(
        s.poll_wait(&[t], &[3], WAIT_AVAILABLE, true),
        Ok(WaitState::Pending)
    );
    let f = s.new_fence();
    s.attach(t, 3, f).unwrap();
    // Available (submitted) but not signalled.
    assert_eq!(
        s.poll_wait(&[t], &[3], WAIT_AVAILABLE, true),
        Ok(WaitState::Ready { first: 0 })
    );
    assert_eq!(
        s.poll_wait(&[t], &[3], WAIT_FOR_SUBMIT, true),
        Ok(WaitState::Pending)
    );
}

#[test]
fn binary_wait_rejects_wait_available_and_unknown_flags() {
    let mut s = Syncobjs::new();
    let h = s.create(CREATE_SIGNALED).unwrap();
    assert_eq!(
        s.poll_wait(&[h], &[], WAIT_AVAILABLE, false),
        Err(SyncError::Invalid)
    );
    assert_eq!(
        s.poll_wait(&[h], &[0], 1 << 7, true),
        Err(SyncError::Invalid)
    );
    assert_eq!(s.create(2), Err(SyncError::Invalid));
}

#[test]
fn empty_wait_is_ready_and_unknown_handles_are_enoent() {
    let mut s = Syncobjs::new();
    assert_eq!(
        s.poll_wait(&[], &[], 0, false),
        Ok(WaitState::Ready { first: 0 })
    );
    assert_eq!(s.poll_wait(&[7], &[], 0, false), Err(SyncError::NoEntry));
    let h = s.create(0).unwrap();
    s.destroy(h).unwrap();
    assert_eq!(s.query(h, 0), Err(SyncError::NoEntry));
    assert_eq!(s.destroy(0), Err(SyncError::NoEntry));
}

#[test]
fn cpu_signal_and_reset() {
    let mut s = Syncobjs::new();
    let (b, t) = (s.create(0).unwrap(), s.create(0).unwrap());
    s.signal(&[b, t], &[0, 4]).unwrap();
    assert_eq!(
        s.poll_wait(&[b], &[], 0, false),
        Ok(WaitState::Ready { first: 0 })
    );
    assert_eq!(s.query(t, 0), Ok(4));
    s.reset(&[b]).unwrap();
    assert_eq!(s.poll_wait(&[b], &[], 0, false), Err(SyncError::Invalid));
    // A bad handle in the array leaves every object unchanged.
    assert_eq!(s.reset(&[t, 99]), Err(SyncError::NoEntry));
    assert_eq!(s.query(t, 0), Ok(4));
}

#[test]
fn query_last_submitted_versus_completed() {
    let mut s = Syncobjs::new();
    let t = s.create(0).unwrap();
    s.signal(&[t], &[1]).unwrap();
    let f = s.new_fence();
    s.attach(t, 6, f).unwrap();
    assert_eq!(s.query(t, 0), Ok(1));
    assert_eq!(s.query(t, QUERY_LAST_SUBMITTED), Ok(6));
    assert_eq!(s.query(t, 2), Err(SyncError::Invalid));
}

#[test]
fn point_zero_replaces_a_timeline_with_a_binary_payload() {
    let mut s = Syncobjs::new();
    let t = s.create(0).unwrap();
    let f = s.new_fence();
    s.attach(t, 5, f).unwrap();
    s.signal(&[t], &[0]).unwrap();
    assert_eq!(
        s.poll_wait(&[t], &[0], 0, true),
        Ok(WaitState::Ready { first: 0 })
    );
    assert_eq!(s.query(t, QUERY_LAST_SUBMITTED), Ok(0));
}

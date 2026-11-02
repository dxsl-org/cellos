use crate::wire::{Binding, Fault, Msg, Refusal, FRAME_LEN};

fn binding() -> Binding {
    Binding {
        caller_cell: 3,
        caller_generation: 9,
        request_id: 77,
        method: 2,
        rights: 0x11,
        payload: 5,
    }
}

#[test]
fn every_message_round_trips() {
    let all = [
        Msg::Attach { nonce: 0xfeed },
        Msg::Restored {
            epoch: 2,
            revision: 3,
        },
        Msg::RestoreFailed { epoch: 2 },
        Msg::Commit {
            epoch: 1,
            expected_revision: 4,
            binding: binding(),
            new_state: 10,
        },
        Msg::Lookup {
            epoch: 1,
            binding: binding(),
        },
        Msg::Attached {
            epoch: 1,
            revision: 2,
            schema: 1,
            counter: 3,
        },
        Msg::Committed {
            revision: 5,
            result: 6,
        },
        Msg::Present {
            revision: 5,
            result: 6,
        },
        Msg::Absent,
        Msg::Refused(Refusal::StaleEpoch),
        Msg::Report,
        Msg::Fence,
        Msg::Corrupt { kind: 3 },
        Msg::Heal,
        Msg::Lend,
        Msg::Lent { grant: 0x8000 },
        Msg::Status {
            state: 1,
            worker: 2,
            epoch: 3,
            revision: 4,
            counter: 5,
            restarts: 6,
            ledger: 7,
            lock_word: 8,
        },
        Msg::Mutate {
            request_id: 9,
            method: 1,
            amount: 2,
            fault: Fault::Stall,
        },
        Msg::Get,
        Msg::Go,
        Msg::Crash {
            mode: 4,
            arg: 0x8000,
        },
        Msg::Done {
            request_id: 1,
            result: 2,
            revision: 3,
            replayed: true,
        },
        Msg::Value {
            counter: 1,
            revision: 2,
            epoch: 3,
        },
        Msg::Staged { request_id: 4 },
        Msg::Failed {
            request_id: 4,
            refusal: Refusal::Conflict,
        },
        Msg::Ack,
    ];
    for msg in all {
        assert_eq!(Msg::decode(&msg.encode()), Some(msg), "{msg:?}");
    }
}

#[test]
fn malformed_frames_are_refused() {
    let good = Msg::Get.encode();
    assert_eq!(Msg::decode(&good[..FRAME_LEN - 1]), None, "short");
    let mut bad = good;
    bad[1] = 2;
    assert_eq!(Msg::decode(&bad), None, "version");
    let mut bad = good;
    bad[3] = 1;
    assert_eq!(Msg::decode(&bad), None, "reserved");
    let mut bad = good;
    bad[0] = 0x7f;
    assert_eq!(Msg::decode(&bad), None, "tag");
    let mut bad = Msg::Refused(Refusal::Denied).encode();
    bad[2] = 0;
    assert_eq!(Msg::decode(&bad), None, "refusal code");
    let mut bad = Msg::Mutate {
        request_id: 1,
        method: 1,
        amount: 1,
        fault: Fault::None,
    }
    .encode();
    bad[5] = 1;
    assert_eq!(Msg::decode(&bad), None, "fault out of range");
    assert_eq!(
        Msg::decode(&[0u8; FRAME_LEN]),
        None,
        "a zero peer-death payload is not a frame"
    );
}

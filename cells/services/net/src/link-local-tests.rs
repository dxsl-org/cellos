use super::*;
use crate::arp::{ArpOp, ArpPacket};
use alloc::vec::Vec;

const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0xc2, 0xc0, 0x01];
const RIVAL: [u8; 6] = [0x52, 0x54, 0x00, 0xc2, 0xc0, 0x99];
/// The service loop's worst-case turn.
const STEP_MS: u64 = 100;

/// Poll every `STEP_MS` over `from..to`, collecting timestamped events.
fn run(engine: &mut LinkLocal, from: u64, to: u64) -> Vec<(u64, Event)> {
    let mut events = drain(engine, from);
    let mut now = from;
    while now < to {
        engine.poll(now);
        events.extend(drain(engine, now));
        now += STEP_MS;
    }
    events
}

fn drain(engine: &mut LinkLocal, now: u64) -> Vec<(u64, Event)> {
    core::iter::from_fn(|| engine.next_event())
        .map(|e| (now, e))
        .collect()
}

fn sent(events: &[(u64, Event)]) -> Vec<(u64, ArpPacket)> {
    events
        .iter()
        .filter_map(|&(at, e)| match e {
            Event::Send(p) => Some((at, p)),
            _ => None,
        })
        .collect()
}

/// An engine that has bound its first candidate by `t = 20 s`.
fn bound_engine() -> (LinkLocal, [u8; 4]) {
    let mut engine = LinkLocal::new(MAC, 0);
    let events = run(&mut engine, 0, 20_000);
    let ip = first_candidate(MAC);
    assert!(events.iter().any(|&(_, e)| e == Event::Bound(ip)));
    (engine, ip)
}

fn arp(op: ArpOp, mac: [u8; 6], sender_ip: [u8; 4], target_ip: [u8; 4]) -> ArpPacket {
    ArpPacket {
        op,
        sender_mac: mac,
        sender_ip,
        target_mac: [0; 6],
        target_ip,
    }
}

#[test]
fn first_candidate_stays_in_the_rfc3927_assignable_range() {
    // The two-node C2C harness expects 169.254.192.{1,2} for these MACs.
    assert_eq!(first_candidate(MAC), [169, 254, 192, 1]);
    for fourth in [0u8, 255] {
        for third in 0..=255u8 {
            let ip = first_candidate([0x52, 0x54, 0x00, third, fourth, 0x07]);
            assert_eq!(&ip[..2], &[169, 254]);
            assert!((1..=254).contains(&ip[2]), "reserved /24 for {ip:?}");
            assert_eq!(ip[3], 0x07);
        }
    }
}

#[test]
fn probes_then_announces_then_binds_on_rfc_timing() {
    let mut engine = LinkLocal::new(MAC, 0);
    let ip = first_candidate(MAC);
    let events = run(&mut engine, 0, 20_000);
    let kinds: Vec<Event> = events.iter().map(|&(_, e)| e).collect();
    let probe = Event::Send(ArpPacket::probe(MAC, ip));
    let announce = Event::Send(ArpPacket::announce(MAC, ip));
    assert_eq!(
        kinds,
        [
            Event::Probing { ip, attempt: 1 },
            probe,
            probe,
            probe,
            Event::Claim(ip),
            announce,
            announce,
            Event::Bound(ip),
        ]
    );
    let frames = sent(&events);
    assert!(frames[0].0 <= PROBE_WAIT_MS);
    for pair in frames[..3].windows(2) {
        let gap = pair[1].0 - pair[0].0;
        assert!(
            (PROBE_MIN_MS..=PROBE_MAX_MS + STEP_MS).contains(&gap),
            "probe gap {gap}"
        );
    }
    assert!(frames[3].0 - frames[2].0 >= ANNOUNCE_WAIT_MS);
    assert!(frames[4].0 - frames[3].0 >= ANNOUNCE_INTERVAL_MS);
    assert_eq!(engine.claimed(), Some(ip));
}

#[test]
fn own_echoed_frames_are_not_conflicts() {
    let mut engine = LinkLocal::new(MAC, 0);
    let ip = first_candidate(MAC);
    run(&mut engine, 0, 1_500);
    engine.on_arp(ArpPacket::probe(MAC, ip), 1_500);
    engine.on_arp(ArpPacket::announce(MAC, ip), 1_500);
    let events = run(&mut engine, 1_500, 20_000);
    assert!(events
        .iter()
        .all(|&(_, e)| !matches!(e, Event::Conflict { .. })));
    assert_eq!(engine.claimed(), Some(ip));
}

#[test]
fn conflict_during_probe_picks_a_new_candidate() {
    let first = first_candidate(MAC);
    for conflicting in [
        arp(ArpOp::Reply, RIVAL, first, [0; 4]),
        ArpPacket::announce(RIVAL, first),
        // Another host probing for the same address at the same time.
        ArpPacket::probe(RIVAL, first),
    ] {
        let mut engine = LinkLocal::new(MAC, 0);
        run(&mut engine, 0, 1_500);
        engine.on_arp(conflicting, 1_500);
        let events = drain(&mut engine, 1_500);
        assert_eq!(
            events[0].1,
            Event::Conflict {
                ip: first,
                mac: RIVAL
            }
        );
        let Event::Probing {
            ip: second,
            attempt: 2,
        } = events[1].1
        else {
            panic!("no second candidate: {events:?}");
        };
        assert_ne!(second, first);
        assert_eq!(&second[..2], &[169, 254]);
        assert!((1..=254).contains(&second[2]));
        let events = run(&mut engine, 1_500, 25_000);
        assert!(events.iter().any(|&(_, e)| e == Event::Bound(second)));
        assert!(events.iter().all(|&(_, e)| e != Event::Claim(first)));
    }
}

#[test]
fn unrelated_arp_is_no_conflict() {
    let first = first_candidate(MAC);
    let mut engine = LinkLocal::new(MAC, 0);
    run(&mut engine, 0, 1_500);
    engine.on_arp(ArpPacket::probe(RIVAL, [169, 254, 192, 2]), 1_500);
    engine.on_arp(arp(ArpOp::Request, RIVAL, [169, 254, 192, 2], first), 1_500);
    let events = run(&mut engine, 1_500, 20_000);
    assert!(events.iter().any(|&(_, e)| e == Event::Bound(first)));
}

#[test]
fn candidate_sequence_is_seeded_from_the_mac() {
    let second = |mac| {
        let mut engine = LinkLocal::new(mac, 0);
        engine.on_arp(ArpPacket::probe(RIVAL, first_candidate(mac)), 0);
        drain(&mut engine, 0).iter().find_map(|&(_, e)| match e {
            Event::Probing { ip, attempt: 2 } => Some(ip),
            _ => None,
        })
    };
    assert_eq!(second(MAC), second(MAC));
    assert_ne!(second(MAC), second([0x52, 0x54, 0x00, 0xc2, 0xc0, 0x02]));
}

#[test]
fn rate_limits_after_max_conflicts() {
    let mut engine = LinkLocal::new(MAC, 0);
    for _ in 0..MAX_CONFLICTS {
        let ip = loop {
            match engine.next_event() {
                Some(Event::Probing { ip, .. }) => break ip,
                Some(_) => {}
                None => panic!("no candidate"),
            }
        };
        engine.on_arp(ArpPacket::announce(RIVAL, ip), 0);
    }
    drain(&mut engine, 0);
    let events = run(&mut engine, 0, RATE_LIMIT_INTERVAL_MS);
    assert!(sent(&events).is_empty(), "probed inside the rate limit");
    let events = run(
        &mut engine,
        RATE_LIMIT_INTERVAL_MS,
        RATE_LIMIT_INTERVAL_MS + 1_000,
    );
    assert_eq!(sent(&events).len(), 1);
}

#[test]
fn bound_address_is_defended_once_then_relinquished() {
    let (mut engine, ip) = bound_engine();
    engine.on_arp(ArpPacket::announce(RIVAL, ip), 30_000);
    assert_eq!(
        drain(&mut engine, 30_000)
            .iter()
            .map(|&(_, e)| e)
            .collect::<Vec<_>>(),
        [
            Event::Send(ArpPacket::announce(MAC, ip)),
            Event::Defended { ip, mac: RIVAL }
        ]
    );
    assert_eq!(engine.claimed(), Some(ip));
    engine.on_arp(
        ArpPacket::announce(RIVAL, ip),
        30_000 + DEFEND_INTERVAL_MS - 1,
    );
    let events = drain(&mut engine, 0);
    assert_eq!(events[0].1, Event::Release { ip, mac: RIVAL });
    assert!(matches!(events[1].1, Event::Probing { attempt: 2, .. }));
    assert_eq!(engine.claimed(), None);
}

#[test]
fn conflicts_further_apart_than_defend_interval_are_each_defended() {
    let (mut engine, ip) = bound_engine();
    for at in [30_000, 30_000 + DEFEND_INTERVAL_MS] {
        engine.on_arp(arp(ArpOp::Reply, RIVAL, ip, [0; 4]), at);
        let events = drain(&mut engine, at);
        assert_eq!(events[1].1, Event::Defended { ip, mac: RIVAL });
    }
    assert_eq!(engine.claimed(), Some(ip));
}

#[test]
fn bound_host_answers_probes_for_its_address() {
    let (mut engine, ip) = bound_engine();
    engine.on_arp(ArpPacket::probe(RIVAL, ip), 30_000);
    let events = drain(&mut engine, 30_000);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].1,
        Event::Send(ArpPacket::reply_to_probe(MAC, ip, RIVAL))
    );
    assert_eq!(engine.claimed(), Some(ip));
}

#[test]
fn announcing_peer_is_logged_once_and_asked_for_its_address() {
    let peer = [169, 254, 192, 2];
    let (mut engine, ip) = bound_engine();
    engine.on_arp(ArpPacket::announce(RIVAL, peer), 30_000);
    engine.on_arp(ArpPacket::announce(RIVAL, peer), 31_000);
    let events: Vec<Event> = drain(&mut engine, 0).iter().map(|&(_, e)| e).collect();
    let request = Event::Send(ArpPacket::request(MAC, ip, peer));
    let logged = Event::Peer {
        ip: peer,
        mac: RIVAL,
        kind: PeerKind::Announce,
    };
    assert_eq!(events, [logged, request, request]);
    // The peer's own answer to its announcement, overheard on a looping
    // segment, is not an answer to us.
    engine.on_arp(arp(ArpOp::Reply, RIVAL, peer, peer), 31_050);
    let elsewhere = ArpPacket {
        target_mac: RIVAL,
        ..arp(ArpOp::Reply, RIVAL, peer, ip)
    };
    engine.on_arp(elsewhere, 31_060);
    assert_eq!(engine.next_event(), None);
    let to_us = ArpPacket {
        target_mac: MAC,
        ..arp(ArpOp::Reply, RIVAL, peer, ip)
    };
    engine.on_arp(to_us, 31_100);
    let reply = Event::Peer {
        ip: peer,
        mac: RIVAL,
        kind: PeerKind::Reply,
    };
    assert_eq!(engine.next_event(), Some(reply));
    // A rebooted peer announces again later and shows up again.
    engine.on_arp(ArpPacket::announce(RIVAL, peer), 30_000 + 10_000);
    assert_eq!(engine.next_event(), Some(logged));
}

#[test]
fn peer_announced_before_our_claim_is_asked_after_it() {
    let peer = [169, 254, 192, 2];
    let mut engine = LinkLocal::new(MAC, 0);
    run(&mut engine, 0, 500);
    engine.on_arp(ArpPacket::announce(RIVAL, peer), 500);
    let events = run(&mut engine, 500, 20_000);
    let ip = first_candidate(MAC);
    let claim = events
        .iter()
        .position(|&(_, e)| e == Event::Claim(ip))
        .unwrap();
    let ask = Event::Send(ArpPacket::request(MAC, ip, peer));
    let asked = events
        .iter()
        .position(|&(_, e)| e == ask)
        .expect("peer never asked");
    assert!(asked > claim);
    assert_eq!(events.iter().filter(|&&(_, e)| e == ask).count(), 1);
}

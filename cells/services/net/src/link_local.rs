//! RFC 3927 IPv4 link-local address claiming: probe, announce, defend.
//!
//! A pure state machine on a millisecond clock. The service loop feeds it the
//! ARP packets the device tapped off its RX path plus the current time, and
//! applies the [`Event`]s it emits (frames to send, the address to configure or
//! drop, log lines). Deadlines are checked once per loop turn, and a turn parks
//! for at most 100 ms, so the second-scale RFC intervals stay within one turn.

use alloc::{collections::VecDeque, vec::Vec};

use crate::arp::{ArpOp, ArpPacket};

/// RFC 3927 §9 constants, in milliseconds where they are times.
pub(crate) const PROBE_WAIT_MS: u64 = 1_000;
pub(crate) const PROBE_NUM: u8 = 3;
pub(crate) const PROBE_MIN_MS: u64 = 1_000;
pub(crate) const PROBE_MAX_MS: u64 = 2_000;
pub(crate) const ANNOUNCE_WAIT_MS: u64 = 2_000;
pub(crate) const ANNOUNCE_NUM: u8 = 2;
pub(crate) const ANNOUNCE_INTERVAL_MS: u64 = 2_000;
pub(crate) const MAX_CONFLICTS: u32 = 10;
pub(crate) const RATE_LIMIT_INTERVAL_MS: u64 = 60_000;
pub(crate) const DEFEND_INTERVAL_MS: u64 = 10_000;
/// The same peer observation is logged at most once per this interval, so a
/// chatty neighbour cannot flood the console but a rebooted one shows again.
const PEER_LOG_INTERVAL_MS: u64 = 10_000;
/// Peers remembered for log de-duplication and pending verification.
const PEER_SLOTS: usize = 8;

/// First candidate, derived from `mac` so a host comes back with the same
/// address (§2.1). Only 169.254.1.0–169.254.254.255 is assignable: the first
/// and last /24 of 169.254/16 are reserved, so a MAC octet of 0 or 255 is remapped.
pub(crate) fn first_candidate(mac: [u8; 6]) -> [u8; 4] {
    let third = match mac[4] {
        0 | 255 => 1 + (mac[3] ^ mac[4]) % 254,
        octet => octet,
    };
    [169, 254, third, mac[5]]
}

/// What a neighbour's ARP packet was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PeerKind {
    Announce,
    Request,
    Reply,
}

/// One step the service loop must apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// Transmit this ARP packet.
    Send(ArpPacket),
    /// Probing starts for `ip`, the `attempt`-th candidate of this run.
    Probing { ip: [u8; 4], attempt: u32 },
    /// Probing found no owner: configure `ip`; announcements follow.
    Claim([u8; 4]),
    /// Announcements done: `ip` is the interface address.
    Bound([u8; 4]),
    /// `mac` uses or probes for the candidate `ip`; another one is chosen.
    Conflict { ip: [u8; 4], mac: [u8; 6] },
    /// `mac` claimed our `ip`; it was defended with one announcement.
    Defended { ip: [u8; 4], mac: [u8; 6] },
    /// `mac` claimed our `ip` again within the defend interval: unconfigure it.
    Release { ip: [u8; 4], mac: [u8; 6] },
    /// A neighbour at link-local `ip`/`mac` was heard.
    Peer {
        ip: [u8; 4],
        mac: [u8; 6],
        kind: PeerKind,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// `sent` probes are out; the next one (after the last: the claim) is due at `due_ms`.
    Probing {
        sent: u8,
        due_ms: u64,
    },
    /// The address is configured; `sent` announcements are out.
    Announcing {
        sent: u8,
        due_ms: u64,
    },
    Bound,
}

struct Peer {
    ip: [u8; 4],
    mac: [u8; 6],
    /// Last log time per [`PeerKind`].
    logged_ms: [Option<u64>; 3],
    /// Announced while we had no address to ask from; asked once we claim one.
    verify: bool,
}

/// One interface's link-local claim.
pub(crate) struct LinkLocal {
    mac: [u8; 6],
    rng: u64,
    candidate: [u8; 4],
    phase: Phase,
    attempt: u32,
    conflicts: u32,
    last_conflict_ms: Option<u64>,
    peers: Vec<Peer>,
    events: VecDeque<Event>,
}

impl LinkLocal {
    /// Start probing the MAC-derived candidate. The pseudo-random sequence for
    /// later candidates and delays is seeded from `mac` too (§2.1), so it
    /// repeats per host and differs between hosts.
    pub(crate) fn new(mac: [u8; 6], now_ms: u64) -> Self {
        let seed = mac.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
        let mut engine = Self {
            mac,
            rng: seed,
            candidate: [0; 4],
            phase: Phase::Bound,
            attempt: 0,
            conflicts: 0,
            last_conflict_ms: None,
            peers: Vec::new(),
            events: VecDeque::new(),
        };
        engine.start_probing(first_candidate(mac), now_ms);
        engine
    }

    /// The configured address (announcing or bound), if any.
    pub(crate) fn claimed(&self) -> Option<[u8; 4]> {
        (!matches!(self.phase, Phase::Probing { .. })).then_some(self.candidate)
    }

    /// Next event to apply, oldest first.
    pub(crate) fn next_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// Advance the probe/announce timers.
    pub(crate) fn poll(&mut self, now_ms: u64) {
        match self.phase {
            Phase::Probing { sent, due_ms } if now_ms >= due_ms && sent < PROBE_NUM => {
                self.send(ArpPacket::probe(self.mac, self.candidate));
                let gap = if sent + 1 < PROBE_NUM {
                    self.random(PROBE_MIN_MS, PROBE_MAX_MS)
                } else {
                    ANNOUNCE_WAIT_MS
                };
                self.phase = Phase::Probing {
                    sent: sent + 1,
                    due_ms: now_ms + gap,
                };
            }
            Phase::Probing { due_ms, .. } if now_ms >= due_ms => {
                self.events.push_back(Event::Claim(self.candidate));
                self.announce_step(0, now_ms);
                self.verify_pending();
            }
            Phase::Announcing { sent, due_ms } if now_ms >= due_ms => {
                self.announce_step(sent, now_ms)
            }
            _ => {}
        }
    }

    /// Account for one ARP packet from the wire (§2.2.1 probing conflicts,
    /// §2.5 defense) and note link-local neighbours.
    pub(crate) fn on_arp(&mut self, packet: ArpPacket, now_ms: u64) {
        // QEMU's multicast netdev (and some switches) echo our own frames back;
        // a packet from our own hardware address is never a conflict.
        if packet.sender_mac == self.mac {
            return;
        }
        let rival_probe = packet.is_probe() && packet.target_ip == self.candidate;
        match self.phase {
            Phase::Probing { .. } if packet.sender_ip == self.candidate || rival_probe => {
                self.conflicts += 1;
                let ip = self.candidate;
                self.events.push_back(Event::Conflict {
                    ip,
                    mac: packet.sender_mac,
                });
                let next = self.next_candidate();
                self.start_probing(next, now_ms);
                return;
            }
            Phase::Announcing { .. } | Phase::Bound if packet.sender_ip == self.candidate => {
                self.defend_or_yield(packet.sender_mac, now_ms);
                return;
            }
            // smoltcp drops requests whose sender is 0.0.0.0, so the prober would
            // never learn the address is taken: answer for it.
            Phase::Announcing { .. } | Phase::Bound if rival_probe => {
                self.send(ArpPacket::reply_to_probe(
                    self.mac,
                    self.candidate,
                    packet.sender_mac,
                ));
            }
            _ => {}
        }
        self.observe_peer(&packet, now_ms);
    }

    fn start_probing(&mut self, ip: [u8; 4], now_ms: u64) {
        self.candidate = ip;
        self.attempt += 1;
        let wait = if self.conflicts >= MAX_CONFLICTS {
            RATE_LIMIT_INTERVAL_MS
        } else {
            self.random(0, PROBE_WAIT_MS)
        };
        self.phase = Phase::Probing {
            sent: 0,
            due_ms: now_ms + wait,
        };
        self.events.push_back(Event::Probing {
            ip,
            attempt: self.attempt,
        });
    }

    fn announce_step(&mut self, sent: u8, now_ms: u64) {
        self.send(ArpPacket::announce(self.mac, self.candidate));
        self.phase = if sent + 1 >= ANNOUNCE_NUM {
            self.events.push_back(Event::Bound(self.candidate));
            Phase::Bound
        } else {
            Phase::Announcing {
                sent: sent + 1,
                due_ms: now_ms + ANNOUNCE_INTERVAL_MS,
            }
        };
    }

    /// §2.5 (b): defend once; a second conflict within DEFEND_INTERVAL gives
    /// the address up and restarts with a new candidate.
    fn defend_or_yield(&mut self, rival: [u8; 6], now_ms: u64) {
        let ip = self.candidate;
        let recent = self
            .last_conflict_ms
            .is_some_and(|at| now_ms.saturating_sub(at) < DEFEND_INTERVAL_MS);
        if recent {
            self.events.push_back(Event::Release { ip, mac: rival });
            self.conflicts += 1;
            self.last_conflict_ms = None;
            let next = self.next_candidate();
            self.start_probing(next, now_ms);
        } else {
            self.last_conflict_ms = Some(now_ms);
            self.send(ArpPacket::announce(self.mac, ip));
            self.events.push_back(Event::Defended { ip, mac: rival });
        }
    }

    /// Log a link-local neighbour, and ask an announcing one for its address:
    /// its reply proves a live stack behind the address and fills our
    /// neighbour cache before the first datagram needs it.
    fn observe_peer(&mut self, packet: &ArpPacket, now_ms: u64) {
        let ip = packet.sender_ip;
        if ip[..2] != [169, 254] || ip == self.candidate {
            return;
        }
        let claimed = self.claimed();
        let kind = match packet.op {
            // Only a reply addressed to us answers a request of ours. Hosts on
            // a looping segment also answer their own announcements, and that
            // overheard reply says nothing about a path back to us.
            ArpOp::Reply if claimed == Some(packet.target_ip) && packet.target_mac == self.mac => {
                PeerKind::Reply
            }
            ArpOp::Reply => return,
            ArpOp::Request if packet.is_announcement() => PeerKind::Announce,
            ArpOp::Request => PeerKind::Request,
        };
        let mac = packet.sender_mac;
        let index = match self.peers.iter().position(|p| p.ip == ip && p.mac == mac) {
            Some(index) => index,
            None => {
                if self.peers.len() >= PEER_SLOTS {
                    self.peers.remove(0);
                }
                self.peers.push(Peer {
                    ip,
                    mac,
                    logged_ms: [None; 3],
                    verify: false,
                });
                self.peers.len() - 1
            }
        };
        let peer = &mut self.peers[index];
        let last = &mut peer.logged_ms[kind as usize];
        if last.is_none_or(|at| now_ms.saturating_sub(at) >= PEER_LOG_INTERVAL_MS) {
            *last = Some(now_ms);
            self.events.push_back(Event::Peer { ip, mac, kind });
        }
        if kind == PeerKind::Announce {
            match claimed {
                Some(own) => self
                    .events
                    .push_back(Event::Send(ArpPacket::request(self.mac, own, ip))),
                None => peer.verify = true,
            }
        }
    }

    fn verify_pending(&mut self) {
        let (mac, own) = (self.mac, self.candidate);
        for peer in self.peers.iter_mut().filter(|p| p.verify) {
            peer.verify = false;
            self.events
                .push_back(Event::Send(ArpPacket::request(mac, own, peer.ip)));
        }
    }

    fn send(&mut self, packet: ArpPacket) {
        self.events.push_back(Event::Send(packet));
    }

    fn next_candidate(&mut self) -> [u8; 4] {
        loop {
            let r = self.next_u64();
            let ip = [169, 254, 1 + (r % 254) as u8, (r >> 32) as u8];
            if ip != self.candidate {
                return ip;
            }
        }
    }

    /// Uniform-enough value in `lo..=hi`.
    fn random(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next_u64() % (hi - lo + 1)
    }

    /// SplitMix64: tiny, and statistically fine for spreading candidates.
    fn next_u64(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
#[path = "link-local-tests.rs"]
mod tests;

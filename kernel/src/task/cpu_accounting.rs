//! Intel development-only charged-occupancy integrator. Lock order is
//! SCHEDULER -> ACCOUNTING; no allocator, console or context switch under ACCOUNTING.
use api::cell_owner::CellOwner;
use super::{scheduler::Scheduler, tcb::Task};
use crate::sync::Spinlock;

const N: usize = 64;
const M: usize = 2;
const EVENTS: usize = 256;
// Reviewed finite development capture policy, never production entitlement.
const CAPTURE_PERIODS: u64 = 18;
const CAPTURE_SWITCHES: usize = 64;
const LIVE: u64 = 1;
const THROTTLED: u64 = 2;
const RETIRING: u64 = 3;
const SEALED: u64 = 4;
const RETIRED: u64 = 5;
const _: [(); N] = [(); crate::memory::cell_quota::MAX_CELLS];
const _: [(); M] = [(); super::smp::MAX_HARTS];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub(crate) enum Error {
    BadConfig = 2, ClockUnavailable, ClockRegression, ArithmeticOverflow,
    ServiceGapExceeded, SwitchDelayExceeded, OvershootExceeded, UnknownOwner,
    StaleGeneration, PolicyConflict, CounterExhausted, WitnessFull,
    SwitchMismatch, Busy, RetirementPending, HistoryFull, ExperimentBoundExceeded,
}
type Result<T, E = Error> = core::result::Result<T, E>;
fn add(a: u64, b: u64) -> Result<u64> { a.checked_add(b).ok_or(Error::ArithmeticOverflow) }
fn mul(a: u64, b: u64) -> Result<u64> { a.checked_mul(b).ok_or(Error::ArithmeticOverflow) }
fn narrow(v: u128) -> Result<u64> { u64::try_from(v).map_err(|_| Error::ArithmeticOverflow) }
fn owner_index(owner: CellOwner) -> Result<usize> {
    let index = usize::try_from(owner.cell_id).map_err(|_| Error::UnknownOwner)?;
    if index == 0 || index >= N || owner.generation == 0 || owner.root_tid == 0 {
        return Err(Error::UnknownOwner);
    }
    Ok(index)
}

macro_rules! words {
    ($name:ident, $count:expr, $($field:ident),+) => {
        #[repr(C)] #[derive(Clone, Copy)]
        pub(crate) struct $name { $(pub $field: u64),+ }
        impl $name {
            const ZERO: Self = Self { $($field: 0),+ };
            pub fn words(self) -> [u64; $count] { [$(self.$field),+] }
        }
        const _: [(); $count * 8] = [(); core::mem::size_of::<$name>()];
    }
}
words!(Group, 24, cell_id, generation, root_tid, state, error, policy_revision,
    entitlement_ns, burst_cap_ns, burst_bank_ns, debt_ns, period_charge_ns,
    lifetime_charge_ns, last_period_charge_ns, last_period_allowance_ns,
    max_period_overshoot_ns, pending_revision, pending_entitlement_ns,
    pending_burst_cap_ns, pending_apply_period, terminal_serial,
    charged_period_count, refusal_count, throttle_count, birth_ns);
words!(Hart, 12, active_cell, active_generation, active_root_tid, active_tid,
    pending_cell, pending_generation, pending_root_tid, pending_tid,
    pending_serial, last_completed_serial, pending_since_ns, last_service_ns);
words!(Header, 18, state, error, boot_session, tick_period_fs, epoch_ns, last_ns,
    period_ns, period_index, next_boundary_ns, control_charge_ns,
    next_switch_serial, event_count, admitted_hart_mask, service_gap_limit_ns,
    switch_delay_limit_ns, exhaustion_to_departure_limit_ns, last_raw_counter,
    timebase_kind);
words!(Event, 8, sequence, now_ns, kind, cell_id, generation, hart, value0, value1);
#[repr(C)]
struct Accounting {
    groups: [Group; N], harts: [Hart; M], header: Header,
    events: [Event; EVENTS], retired: [Group; N], retired_count: u64,
}
const _: [(); 41304] = [(); core::mem::size_of::<Accounting>()];
static ACCOUNTING: Spinlock<Accounting> = Spinlock::new(Accounting {
    groups: [Group::ZERO; N], harts: [Hart::ZERO; M], header: Header::ZERO,
    events: [Event::ZERO; EVENTS], retired: [Group::ZERO; N], retired_count: 0,
});

#[derive(Clone, Copy)]
pub(crate) struct Gate { generation: u64, root: u64, runnable: bool }
impl Gate { const CONTROL: Self = Self { generation: 0, root: 0, runnable: true }; }
pub(crate) struct Eligibility {
    gates: [Gate; N],
    workers: [usize; M],
    mask: u64,
}
impl Eligibility {
    pub fn permits_on(&self, task: &Task, hart: usize) -> bool {
        let Some(g) = self.gates.get(task.cell_id.0 as usize) else { return false };
        if g.generation == 0 { return true }
        if g.generation != task.cell_generation || g.root != task.root_tid as u64 || !g.runnable {
            return false;
        }
        if task.id == task.root_tid { return true }
        let Some(rank) = self.workers.iter().position(|id| *id == task.id) else { return false };
        hart == if self.mask.count_ones() == 1 { 0 } else { rank }
    }
}

impl Accounting {
    fn emit(&mut self, kind: u64, now: u64, cell: u64, generation: u64,
        hart: u64, value0: u64, value1: u64) -> Result<()> {
        let index = self.header.event_count as usize;
        if index >= EVENTS { return Err(Error::WitnessFull) }
        self.events[index] = Event { sequence: index as u64, now_ns: now, kind,
            cell_id: cell, generation, hart, value0, value1 };
        self.header.event_count += 1;
        Ok(())
    }
    fn seal(&mut self, error: Error) {
        if self.header.error != 0 { return }
        self.header.error = error as u64;
        self.header.state = 2;
        for g in &mut self.groups {
            if g.state != 0 { g.state = SEALED; g.error = error as u64; }
        }
        let _ = self.emit(11, self.header.last_ns, 0, 0, u64::MAX,
            error as u64, self.header.last_raw_counter);
    }
    fn reserve_events(&self, count: usize) -> Result<()> {
        if self.header.event_count as usize > EVENTS - count.min(EVENTS) || count > EVENTS {
            return Err(Error::WitnessFull);
        }
        Ok(())
    }
    fn append_reserved(&mut self, mut event: Event) {
        let index = self.header.event_count as usize;
        event.sequence = index as u64;
        self.events[index] = event;
        self.header.event_count += 1;
    }
    fn check_clock_identity(&self, clock: &crate::hal::hpet::CheckedSample) -> Result<()> {
        // The first two journal slots are immutable, non-evicting admission
        // records. Retain the whole checked tuple without enlarging the payload.
        if self.header.event_count < 2 || self.events[0].kind != 1 || self.events[1].kind != 16
            || clock.period_fs != self.header.tick_period_fs
            || clock.base as u64 != self.events[0].value0
            || clock.capabilities != self.events[0].value1
            || clock.configuration != self.events[1].value0 {
            return Err(Error::ClockUnavailable);
        }
        Ok(())
    }
    fn group_index(&self, cell: u64, generation: u64, root: u64) -> Result<usize> {
        let i = usize::try_from(cell).map_err(|_| Error::UnknownOwner)?;
        let g = self.groups.get(i).ok_or(Error::UnknownOwner)?;
        if cell == 0 || g.state == 0 { return Err(Error::UnknownOwner) }
        if g.generation != generation || g.root_tid != root { return Err(Error::StaleGeneration) }
        if !(LIVE..=RETIRED).contains(&g.state) { return Err(Error::UnknownOwner) }
        Ok(i)
    }
    fn entitlement(&self, g: &Group) -> Result<u64> {
        let start = self.header.next_boundary_ns - self.header.period_ns;
        if g.birth_ns > start {
            narrow((g.entitlement_ns as u128) *
                ((self.header.next_boundary_ns - g.birth_ns) as u128) /
                self.header.period_ns as u128)
        } else { Ok(g.entitlement_ns) }
    }
    fn allowance(&self, g: &Group) -> Result<u64> {
        let credit = add(self.entitlement(g)?, g.burst_bank_ns)?;
        Ok(credit.saturating_sub(g.debt_ns))
    }
    fn occupied(&self, cell: u64, generation: u64) -> bool {
        self.harts.iter().any(|h| (h.active_cell == cell && h.active_generation == generation)
            || (h.pending_serial != 0 && h.pending_cell == cell && h.pending_generation == generation))
    }
    // Before terminal retention, terminal_serial holds exhaustion-ns+1.
    // It survives replenishment and clears only on final exact-owner departure;
    // retirement then replaces it with the terminal switch serial.
    fn check_departure(&self, now: u64) -> Result<()> {
        for g in &self.groups {
            if g.state != 0 && g.state != RETIRED && g.terminal_serial != 0
                && self.occupied(g.cell_id, g.generation)
                && now.checked_sub(g.terminal_serial - 1).ok_or(Error::ClockRegression)?
                    > self.header.exhaustion_to_departure_limit_ns {
                return Err(Error::OvershootExceeded);
            }
        }
        Ok(())
    }
    fn segment(&mut self, end: u64) -> Result<()> {
        let start = self.header.last_ns;
        let dt = end.checked_sub(start).ok_or(Error::ClockRegression)?;
        let mut charge = [0u64; N];
        let mut control = 0;
        for hart in 0..M {
            if self.header.admitted_hart_mask & (1 << hart) == 0 { continue }
            let h = self.harts[hart];
            if h.active_cell == 0 { control = add(control, dt)?; }
            else {
                let i = self.group_index(h.active_cell, h.active_generation, h.active_root_tid)?;
                charge[i] = add(charge[i], dt)?;
            }
        }
        let control_total = add(self.header.control_charge_ns, control)?;
        // At most two occupied groups change. Retain only those candidate rows,
        // not a copy of the accounting store or a 64-row stack transaction.
        let mut rows = [Group::ZERO; M];
        let mut indices = [0usize; M];
        let mut events = [Event::ZERO; M];
        let mut changed = 0;
        let mut event_count = 0;
        self.check_departure(end)?;
        for i in 1..N {
            if charge[i] == 0 { continue }
            let mut g = self.groups[i];
            let allowance = self.allowance(&g)?;
            let before = g.period_charge_ns;
            g.period_charge_ns = add(before, charge[i])?;
            let bound = add(allowance, mul(self.header.admitted_hart_mask.count_ones() as u64,
                self.header.exhaustion_to_departure_limit_ns)?)?;
            if g.period_charge_ns > bound { return Err(Error::OvershootExceeded) }
            g.lifetime_charge_ns = add(g.lifetime_charge_ns, charge[i])?;
            g.max_period_overshoot_ns = g.max_period_overshoot_ns.max(g.period_charge_ns.saturating_sub(allowance));
            if g.period_charge_ns >= allowance && (g.state == LIVE
                || (g.state == RETIRING && before < allowance)
                || (g.state == THROTTLED && g.terminal_serial == 0)) {
                let mut mask = 0;
                let mut occupancy = 0;
                for (hart, h) in self.harts.iter().enumerate() {
                    if h.active_cell == g.cell_id && h.active_generation == g.generation { occupancy += 1; mask |= 1 << hart; }
                    if h.pending_serial != 0 && h.pending_cell == g.cell_id && h.pending_generation == g.generation { mask |= 1 << hart; }
                }
                let exhausted = if allowance > before { add(start, (allowance-before).div_ceil(occupancy))? } else { start };
                if g.state == LIVE { g.state = THROTTLED; }
                g.throttle_count = add(g.throttle_count, 1)?;
                if g.terminal_serial == 0 { g.terminal_serial = add(exhausted, 1)?; }
                if end - (g.terminal_serial-1) > self.header.exhaustion_to_departure_limit_ns {
                    return Err(Error::OvershootExceeded);
                }
                events[event_count] = Event { now_ns: exhausted, kind: 7, cell_id: g.cell_id,
                    generation: g.generation, hart: u64::MAX, value0: allowance, value1: mask, ..Event::ZERO };
                event_count += 1;
            }
            indices[changed] = i;
            rows[changed] = g;
            changed += 1;
        }
        self.reserve_events(event_count)?;
        // No fallible operation remains after the coherent segment commit.
        for i in 0..changed { self.groups[indices[i]] = rows[i]; }
        self.header.control_charge_ns = control_total;
        self.header.last_ns = end;
        for event in events.into_iter().take(event_count) { self.append_reserved(event); }
        Ok(())
    }
    fn settled_row(&self, mut g: Group) -> Result<(Group, Event)> {
        let credit = add(self.entitlement(&g)?, g.burst_bank_ns)?;
        let debit = add(g.debt_ns, g.period_charge_ns)?;
        let allowance = credit.saturating_sub(g.debt_ns);
        let event = Event { now_ns: self.header.next_boundary_ns, kind: 6, cell_id: g.cell_id,
            generation: g.generation, hart: u64::MAX, value0: g.period_charge_ns, value1: allowance, ..Event::ZERO };
        g.last_period_charge_ns = g.period_charge_ns;
        g.last_period_allowance_ns = allowance;
        if g.period_charge_ns != 0 { g.charged_period_count = add(g.charged_period_count, 1)?; }
        if g.pending_revision != 0 {
            if g.pending_apply_period != add(self.header.period_index, 1)? { return Err(Error::PolicyConflict) }
            g.policy_revision = g.pending_revision;
            g.entitlement_ns = g.pending_entitlement_ns;
            g.burst_cap_ns = g.pending_burst_cap_ns;
            g.pending_revision = 0; g.pending_entitlement_ns = 0;
            g.pending_burst_cap_ns = 0; g.pending_apply_period = 0;
        }
        g.burst_bank_ns = credit.saturating_sub(debit).min(g.burst_cap_ns);
        g.debt_ns = debit.saturating_sub(credit);
        g.period_charge_ns = 0;
        if g.state == THROTTLED || g.state == LIVE {
            g.state = if add(g.entitlement_ns, g.burst_bank_ns)? > g.debt_ns { LIVE } else { THROTTLED };
        }
        Ok((g, event))
    }
    fn settle(&mut self) -> Result<()> {
        let period = add(self.header.period_index, 1)?;
        let boundary = add(self.header.next_boundary_ns, self.header.period_ns)?;
        let mut count = 0;
        // Read-only preflight keeps the old common boundary and all old rows
        // intact on any arithmetic, policy or witness-capacity refusal.
        for g in self.groups.iter().filter(|g| g.state != 0) {
            self.settled_row(*g)?;
            count += 1;
        }
        self.reserve_events(count)?;
        for i in 1..N {
            if self.groups[i].state == 0 { continue }
            // Same immutable row/boundary as preflight; no allocation or clock
            // sampling intervenes, so this deterministic calculation cannot fail.
            let (row, event) = self.settled_row(self.groups[i]).expect("preflighted period row");
            self.groups[i] = row;
            self.append_reserved(event);
        }
        self.header.period_index = period;
        self.header.next_boundary_ns = boundary;
        Ok(())
    }
    fn empty_period_row(&self, mut g: Group, q: u64, end: u64) -> Result<(Group, Event)> {
        let before_last = g.burst_bank_ns as u128 + (q-1) as u128 * g.entitlement_ns as u128;
        let debt = g.debt_ns as u128;
        let last_bank = narrow(before_last.saturating_sub(debt).min(g.burst_cap_ns as u128))?;
        let last_debt = narrow(debt.saturating_sub(before_last))?;
        let last_allowance = add(g.entitlement_ns, last_bank)?.saturating_sub(last_debt);
        let credit = g.burst_bank_ns as u128 + q as u128 * g.entitlement_ns as u128;
        g.burst_bank_ns = narrow(credit.saturating_sub(debt).min(g.burst_cap_ns as u128))?;
        g.debt_ns = narrow(debt.saturating_sub(credit))?;
        g.last_period_charge_ns = 0;
        g.last_period_allowance_ns = last_allowance;
        if g.state == LIVE || g.state == THROTTLED {
            g.state = if add(g.entitlement_ns, g.burst_bank_ns)? > g.debt_ns { LIVE } else { THROTTLED };
        }
        Ok((g, Event { now_ns: end, kind: 14, cell_id: g.cell_id, generation: g.generation,
            hart: u64::MAX, value0: q, value1: last_allowance, ..Event::ZERO }))
    }
    fn service_summary(&mut self, hart: usize, continuation: bool, incoming_workload: bool) -> Result<()> {
        let h = *self.harts.get(hart).ok_or(Error::BadConfig)?;
        let workload = h.active_cell != 0 || (h.pending_serial != 0 && h.pending_cell != 0);
        if !workload && !incoming_workload { return Ok(()) }
        let gap = if workload {
            self.header.last_ns.checked_sub(h.last_service_ns).ok_or(Error::ClockRegression)?
        } else { 0 };
        if gap > self.header.service_gap_limit_ns { return Err(Error::ServiceGapExceeded) }
        let existing = self.events[..self.header.event_count as usize].iter().position(|e|
            e.kind == 17 && e.hart == hart as u64 && e.generation == self.header.period_index);
        if let Some(index) = existing {
            let count = add(self.events[index].value1, u64::from(continuation))?;
            self.events[index].value0 = self.events[index].value0.max(gap);
            self.events[index].value1 = count;
            self.events[index].now_ns = self.header.last_ns;
        } else {
            self.emit(17, self.header.last_ns, 0, self.header.period_index,
                hart as u64, gap, u64::from(continuation))?;
        }
        Ok(())
    }
    fn advance(&mut self) -> Result<()> {
        if self.header.state == 0 || self.header.state >= 3 { return Ok(()) }
        if self.header.state != 1 { return Err(Error::Busy) }
        let clock = crate::hal::hpet::checked_sample().map_err(|_| Error::ClockUnavailable)?;
        self.check_clock_identity(&clock)?;
        if clock.raw_counter < self.header.last_raw_counter { return Err(Error::ClockRegression) }
        let now = narrow(clock.raw_counter as u128 * clock.period_fs as u128 / 1_000_000)?;
        let gap = now.checked_sub(self.header.last_ns).ok_or(Error::ClockRegression)?;
        if now.checked_sub(self.header.epoch_ns).ok_or(Error::ClockRegression)?
            >= mul(self.header.period_ns, CAPTURE_PERIODS)? {
            return Err(Error::ExperimentBoundExceeded);
        }
        let occupied = self.harts.iter().any(|h| h.active_cell != 0 || (h.pending_serial != 0 && h.pending_cell != 0));
        if occupied && gap > self.header.period_ns { return Err(Error::ServiceGapExceeded) }
        for (hart, h) in self.harts.iter().enumerate() {
            if self.header.admitted_hart_mask & (1 << hart) == 0 { continue }
            if (h.active_cell != 0 || (h.pending_serial != 0 && h.pending_cell != 0))
                && now.checked_sub(h.last_service_ns).ok_or(Error::ClockRegression)? > self.header.service_gap_limit_ns {
                return Err(Error::ServiceGapExceeded)
            }
            if h.pending_serial != 0 && now.checked_sub(h.pending_since_ns).ok_or(Error::ClockRegression)?
                > self.header.switch_delay_limit_ns { return Err(Error::SwitchDelayExceeded) }
        }
        self.header.last_raw_counter = clock.raw_counter;
        if now >= self.header.next_boundary_ns {
            self.segment(self.header.next_boundary_ns)?;
            self.settle()?;
            if now >= self.header.next_boundary_ns {
                // Only fully empty periods reach this analytical shortcut.
                let q = add((now - self.header.next_boundary_ns) / self.header.period_ns, 1)?;
                let skip = mul(q, self.header.period_ns)?;
                let end = add(self.header.last_ns, skip)?;
                let period = add(self.header.period_index, q)?;
                let boundary = add(self.header.next_boundary_ns, skip)?;
                let control = add(self.header.control_charge_ns,
                    mul(skip, self.header.admitted_hart_mask.count_ones() as u64)?)?;
                let mut count = 0;
                for g in self.groups.iter().filter(|g| g.state != 0) {
                    self.empty_period_row(*g, q, end)?;
                    count += 1;
                }
                self.reserve_events(count)?;
                for i in 1..N {
                    if self.groups[i].state == 0 { continue }
                    let (row, event) = self.empty_period_row(self.groups[i], q, end)
                        .expect("preflighted empty period row");
                    self.groups[i] = row;
                    self.append_reserved(event);
                }
                self.header.control_charge_ns = control;
                self.header.last_ns = end;
                self.header.period_index = period;
                self.header.next_boundary_ns = boundary;
            }
        }
        self.segment(now)
    }
    fn enable(&mut self, boot_session: u64) -> Result<()> {
        if self.header.state != 0 { return Err(Error::Busy) }
        let clock = crate::hal::hpet::checked_sample().map_err(|_| Error::ClockUnavailable)?;
        let now = narrow(clock.raw_counter as u128 * clock.period_fs as u128 / 1_000_000)?;
        let m = super::smp::online_hart_count();
        if m == 0 || m > M || boot_session == 0 { return Err(Error::BadConfig) }
        for hart in 0..m {
            if super::hart_local::ready::selected_task_id_for(hart) != 0
                || super::hart_local::ready::outgoing_context_save_task_id_for(hart) != 0 {
                return Err(Error::Busy);
            }
        }
        self.header = Header { state: 1, boot_session, tick_period_fs: clock.period_fs,
            epoch_ns: now, last_ns: now, period_ns: 100_000_000, period_index: 0,
            next_boundary_ns: add(now, 100_000_000)?, admitted_hart_mask: (1 << m) - 1,
            service_gap_limit_ns: 20_000_000, switch_delay_limit_ns: 20_000_000,
            exhaustion_to_departure_limit_ns: 20_000_000, next_switch_serial: 1,
            last_raw_counter: clock.raw_counter, timebase_kind: 1, ..Header::ZERO };
        for hart in 0..m {
            self.harts[hart].active_tid = super::hart_local::ready::executing_task_id_for(hart) as u64;
            self.harts[hart].last_service_ns = now;
        }
        self.emit(1, now, 0, 0, u64::MAX, clock.base as u64, clock.capabilities)?;
        self.emit(16, now, 0, 0, u64::MAX, clock.configuration, clock.raw_counter)
    }
    fn tuple(&self, task: Option<&Task>) -> Result<(u64, u64, u64, u64)> {
        let Some(task) = task else { return Ok((0, 0, 0, 0)) };
        let i = task.cell_id.0 as usize;
        if i >= N { return Err(Error::UnknownOwner) }
        if self.groups[i].state == 0 { return Ok((0, 0, 0, task.id as u64)) }
        self.group_index(task.cell_id.0, task.cell_generation, task.root_tid as u64)?;
        Ok((task.cell_id.0, task.cell_generation, task.root_tid as u64, task.id as u64))
    }
}

// All entry points carry the existing scheduler reference as their lock-order token.
fn mutate<T>(_: &Scheduler, f: impl FnOnce(&mut Accounting) -> Result<T>) -> Result<T> {
    let mut a = ACCOUNTING.lock();
    let result = f(&mut a);
    if let Err(e) = result { a.seal(e); }
    let mut preempt = 0;
    for (hart, h) in a.harts.iter().enumerate() {
        for cell in [h.active_cell, h.pending_cell] {
            if cell != 0 && a.groups.get(cell as usize).is_none_or(|g|
                g.state != LIVE || g.terminal_serial != 0) { preempt |= 1 << hart; }
        }
    }
    drop(a);
    for hart in 0..M { if preempt & (1 << hart) != 0 { super::smp::request_preemption(hart); } }
    result
}
pub(crate) fn fatal(error: Error) -> ! {
    // ACCOUNTING is dropped; the caller may still hold SCHEDULER. No console
    // polling, allocation or retirement is permitted on the incoming failure path.
    unsafe { core::arch::asm!("cli"); }
    unsafe { core::arch::asm!("out dx, eax", in("dx") 0xf4u16, in("eax") 0x20u32 + error as u32); }
    loop { unsafe { core::arch::asm!("hlt"); } }
}
pub(crate) fn require<T>(r: Result<T>) -> T { match r { Ok(v) => v, Err(e) => fatal(e) } }

pub(crate) fn bind(s: &Scheduler, owner: CellOwner) -> Result<()> {
    mutate(s, |a| {
        let i = owner.cell_id as usize;
        if i == 0 || i >= N || owner.generation == 0 || owner.root_tid == 0 { return Err(Error::BadConfig) }
        if a.header.state == 0 { a.enable(owner.root_tid)?; }
        a.advance()?;
        if owner.root_tid != a.header.boot_session { return Err(Error::ExperimentBoundExceeded) }
        if a.groups[i].state != 0 || a.retired_count >= N as u64 { return Err(Error::HistoryFull) }
        a.emit(2, a.header.last_ns, owner.cell_id, owner.generation, u64::MAX, owner.root_tid, 20_000_000)?;
        a.groups[i] = Group { cell_id: owner.cell_id, generation: owner.generation,
            root_tid: owner.root_tid, state: LIVE, policy_revision: 1,
            entitlement_ns: 20_000_000, burst_cap_ns: 40_000_000,
            birth_ns: a.header.last_ns, ..Group::ZERO };
        Ok(())
    })
}
pub(crate) fn inherit(s: &Scheduler, owner: CellOwner) -> Result<()> {
    mutate(s, |a| {
        let slot = owner_index(owner)?;
        if a.header.state == 0 || a.groups[slot].state == 0 { return Ok(()) }
        a.advance()?;
        let i = a.group_index(owner.cell_id, owner.generation, owner.root_tid)?;
        if a.groups[i].state != LIVE && a.groups[i].state != THROTTLED { return Err(Error::RetirementPending) }
        Ok(())
    })
}

/// Release only the selected root's final private measurement wait, using the
/// checked charge clock rather than assuming scheduler ticks equal HPET time.
/// Caller publishes through the existing mailbox after ACCOUNTING is dropped.
pub(crate) fn period_wait_target(s: &Scheduler) -> Result<Option<usize>> {
    mutate(s, |a| {
        a.advance()?;
        if a.header.state != 1 || a.header.period_index < 16 { return Ok(None) }
        let tid = usize::try_from(a.header.boot_session).map_err(|_| Error::UnknownOwner)?;
        let Some(task) = s.tasks.get(&tid) else { return Ok(None) };
        let i = a.group_index(task.cell_id.0, task.cell_generation, task.root_tid as u64)?;
        if a.groups[i].state != LIVE && a.groups[i].state != THROTTLED { return Ok(None) }
        if !matches!(task.state, super::tcb::TaskState::Recv {
            mask: 0, deadline: None, buf_len: 2, ..
        }) || !task.pending_msgs.is_empty() {
            return Ok(None);
        }
        if s.tasks.values().any(|member| member.id != tid
            && member.cell_id.0 == task.cell_id.0
            && member.cell_generation == task.cell_generation) {
            return Ok(None);
        }
        Ok(Some(tid))
    })
}

/// Let a measured worker exit only after the selected root's real Wait has
/// registered its waiter. This avoids losing an already-reaped exit outcome;
/// the ordinary quiescent reaper still publishes the actual exit code.
pub(crate) fn join_wait_target(s: &Scheduler) -> Result<Option<usize>> {
    mutate(s, |a| {
        a.advance()?;
        if a.header.state != 1 { return Ok(None) }
        let root = usize::try_from(a.header.boot_session).map_err(|_| Error::UnknownOwner)?;
        let Some(parent) = s.tasks.get(&root) else { return Ok(None) };
        let super::tcb::TaskState::Waiting { target } = parent.state else { return Ok(None) };
        let Some(worker) = s.tasks.get(&target) else { return Ok(None) };
        if worker.id == root || worker.root_tid != root
            || worker.cell_id != parent.cell_id
            || worker.cell_generation != parent.cell_generation
            || !worker.waiters.contains(&root)
            || !matches!(worker.state, super::tcb::TaskState::Recv {
                mask: 0, deadline: None, buf_len: 2, ..
            }) || !worker.pending_msgs.is_empty() {
            return Ok(None);
        }
        a.group_index(worker.cell_id.0, worker.cell_generation, root as u64)?;
        Ok(Some(target))
    })
}

/// Release compute only after both exact-generation workers have consumed their
/// attested root message and entered the three-byte receive.
pub(crate) fn start_wait_targets(s: &Scheduler) -> Result<([Option<usize>; M], u8)> {
    mutate(s, |a| {
        a.advance()?;
        if a.header.state != 1 { return Ok(([None; M], 0)) }
        let root = a.header.boot_session as usize;
        let Some(parent) = s.tasks.get(&root) else { return Ok(([None; M], 0)) };
        a.group_index(parent.cell_id.0, parent.cell_generation, parent.root_tid as u64)?;
        let mut targets = [None; M];
        let mut count = 0;
        for worker in s.tasks.values().filter(|task| task.id != root
            && task.root_tid == root && task.cell_id == parent.cell_id
            && task.cell_generation == parent.cell_generation) {
            if count >= M { return Err(Error::ExperimentBoundExceeded) }
            if !matches!(worker.state, super::tcb::TaskState::Recv {
                mask: 0, deadline: None, buf_len: 3, ..
            }) || !worker.pending_msgs.is_empty() {
                return Ok(([None; M], 0));
            }
            targets[count] = Some(worker.id);
            count += 1;
        }
        if count != M { return Ok(([None; M], 0)) }
        Ok((targets, a.header.admitted_hart_mask.count_ones() as u8))
    })
}
pub(crate) fn service(s: &Scheduler, hart: usize) -> Result<()> {
    mutate(s, |a| {
        a.advance()?;
        if a.header.state == 1 {
            if hart >= M || a.header.admitted_hart_mask & (1 << hart) == 0 { return Err(Error::BadConfig) }
            a.service_summary(hart, false, false)?;
            a.harts[hart].last_service_ns = a.header.last_ns;
        }
        Ok(())
    })
}
pub(crate) fn eligibility(s: &Scheduler) -> Result<Eligibility> {
    mutate(s, |a| {
        a.advance()?;
        let mut gates = [Gate::CONTROL; N];
        for (i, g) in a.groups.iter().enumerate() {
            if g.state != 0 { gates[i] = Gate { generation: g.generation, root: g.root_tid,
                runnable: a.header.state == 1 && g.state == LIVE
                    && g.terminal_serial == 0 && g.period_charge_ns < a.allowance(g)? }; }
        }
        let mut workers = [0; M];
        let mut count = 0;
        for task in s.tasks.values() {
            if task.id == task.root_tid { continue }
            let Some(group) = a.groups.get(task.cell_id.0 as usize) else { return Err(Error::UnknownOwner) };
            if group.state == 0 { continue }
            a.group_index(task.cell_id.0, task.cell_generation, task.root_tid as u64)?;
            if count >= M { return Err(Error::ExperimentBoundExceeded) }
            workers[count] = task.id;
            count += 1;
        }
        Ok(Eligibility { gates, workers, mask: a.header.admitted_hart_mask })
    })
}
pub(crate) fn no_switch(s: &Scheduler, hart: usize, task: Option<&Task>) -> Result<()> {
    mutate(s, |a| {
        a.advance()?;
        if a.header.state != 1 { return Ok(()) }
        let expected = a.tuple(task)?;
        let h = *a.harts.get(hart).ok_or(Error::BadConfig)?;
        if h.pending_serial != 0 || expected != (h.active_cell, h.active_generation, h.active_root_tid, h.active_tid) {
            return Err(Error::SwitchMismatch)
        }
        a.service_summary(hart, expected.0 != 0, false)?;
        a.harts[hart].last_service_ns = a.header.last_ns;
        Ok(())
    })
}
pub(crate) fn stage(s: &Scheduler, hart: usize, incoming: Option<&Task>) -> Result<bool> {
    mutate(s, |a| {
        a.advance()?;
        if a.header.state != 1 { return Ok(true) }
        let tuple = a.tuple(incoming)?;
        let h = *a.harts.get(hart).ok_or(Error::BadConfig)?;
        if h.pending_serial != 0
            || h.active_tid != super::hart_local::ready::executing_task_id_for(hart) as u64 {
            return Err(Error::SwitchMismatch);
        }
        if tuple.0 != 0 {
            let i = a.group_index(tuple.0, tuple.1, tuple.2)?;
            // Ordinary stale eligibility refuses without sealing, before task
            // state, current identity, or ready ownership pins are published.
            if a.groups[i].state != LIVE || a.groups[i].terminal_serial != 0
                || a.groups[i].period_charge_ns >= a.allowance(&a.groups[i])? {
                return Ok(false);
            }
        }
        if tuple.3 != 0 && a.harts.iter().enumerate().any(|(other, h)|
            other != hart && (h.active_tid == tuple.3
                || (h.pending_serial != 0 && h.pending_tid == tuple.3))) {
            return Err(Error::SwitchMismatch);
        }
        let serial = a.header.next_switch_serial;
        let next_serial = serial.checked_add(1).ok_or(Error::CounterExhausted)?;
        let workload = h.active_cell != 0 || tuple.0 != 0;
        if workload && a.events[..a.header.event_count as usize].iter()
            .filter(|e| e.kind == 3).count() >= CAPTURE_SWITCHES {
            return Err(Error::ExperimentBoundExceeded);
        }
        a.reserve_events(if workload { 2 } else { 0 })?;
        a.service_summary(hart, false, tuple.0 != 0)?;
        if workload {
            a.append_reserved(Event { now_ns: a.header.last_ns, kind: 3,
                cell_id: tuple.0, generation: tuple.1, hart: hart as u64,
                value0: tuple.3, value1: serial, ..Event::ZERO });
        }
        a.header.next_switch_serial = next_serial;
        let h = &mut a.harts[hart];
        h.pending_cell = tuple.0; h.pending_generation = tuple.1;
        h.pending_root_tid = tuple.2; h.pending_tid = tuple.3;
        h.pending_serial = serial; h.pending_since_ns = a.header.last_ns;
        h.last_service_ns = a.header.last_ns;
        Ok(true)
    })
}
pub(crate) fn complete(s: &Scheduler, hart: usize, selected: usize) -> Result<()> {
    mutate(s, |a| {
        a.advance()?;
        if a.header.state != 1 { return Ok(()) }
        let h = *a.harts.get(hart).ok_or(Error::BadConfig)?;
        if h.pending_serial == 0 || h.pending_tid != selected as u64
            || h.pending_serial <= h.last_completed_serial { return Err(Error::SwitchMismatch) }
        if h.pending_cell != 0 { a.group_index(h.pending_cell, h.pending_generation, h.pending_root_tid)?; }
        a.reserve_events(if h.active_cell != 0 || h.pending_cell != 0 { 2 } else { 0 })?;
        a.service_summary(hart, false, h.pending_cell != 0)?;
        if h.active_cell != 0 || h.pending_cell != 0 {
            a.emit(4, a.header.last_ns, h.active_cell, h.active_generation,
                hart as u64, h.active_tid, h.pending_serial)?;
        }
        a.harts[hart] = Hart { active_cell: h.pending_cell, active_generation: h.pending_generation,
            active_root_tid: h.pending_root_tid, active_tid: h.pending_tid,
            last_completed_serial: h.pending_serial, last_service_ns: a.header.last_ns, ..Hart::ZERO };
        if h.active_cell != 0 {
            let i = a.group_index(h.active_cell, h.active_generation, h.active_root_tid)?;
            if !a.occupied(h.active_cell, h.active_generation) {
                a.groups[i].terminal_serial = 0;
            }
        }
        Ok(())
    })
}
/// # Safety
/// Only the scheduler's pre-publication domain-pin refusal may call this:
/// the staged raw switch has not been attempted.
pub(crate) unsafe fn abort_pending_before_raw(s: &Scheduler, hart: usize) -> Result<()> {
    let serial = { let a = ACCOUNTING.lock();
        if a.header.state == 0 { return Ok(()) }
        a.harts.get(hart).ok_or(Error::BadConfig)?.pending_serial
    };
    unsafe { abort_staged_switch(s, hart, serial) }
}

/// # Safety
/// Caller must prove the committed raw switch has never been attempted. This
/// cannot be inferred from task state or an unacknowledged completion alone.
#[allow(dead_code)]
pub(crate) unsafe fn abort_staged_switch(s: &Scheduler, hart: usize, serial: u64) -> Result<()> {
    mutate(s, |a| {
        a.advance()?;
        if a.header.state != 1 { return Ok(()) }
        let h = *a.harts.get(hart).ok_or(Error::BadConfig)?;
        if serial == 0 || h.pending_serial != serial
            || super::hart_local::ready::executing_task_id_for(hart) as u64 != h.active_tid {
            return Err(Error::SwitchMismatch);
        }
        if h.active_cell != 0 || h.pending_cell != 0 {
            a.emit(15, a.header.last_ns, h.pending_cell, h.pending_generation,
                hart as u64, h.pending_tid, serial)?;
        }
        a.harts[hart].pending_cell = 0; a.harts[hart].pending_generation = 0;
        a.harts[hart].pending_root_tid = 0; a.harts[hart].pending_tid = 0;
        a.harts[hart].pending_serial = 0; a.harts[hart].pending_since_ns = 0;
        Ok(())
    })
}

pub(crate) fn begin_retirement(s: &Scheduler, owner: CellOwner) -> Result<()> {
    mutate(s, |a| {
        let slot = owner_index(owner)?;
        if a.header.state == 0 || a.groups[slot].state == 0 { return Ok(()) }
        a.advance()?;
        let i = a.group_index(owner.cell_id, owner.generation, owner.root_tid)?;
        if a.groups[i].state == RETIRING { return Ok(()) }
        a.emit(9, a.header.last_ns, owner.cell_id, owner.generation, u64::MAX, owner.root_tid, 0)?;
        a.groups[i].state = RETIRING;
        Ok(())
    })
}
pub(crate) fn quiescent(s: &Scheduler, owner: CellOwner) -> bool {
    let _ = s;
    let a = ACCOUNTING.lock();
    a.header.state == 0 || (a.header.state == 1 && !a.occupied(owner.cell_id, owner.generation))
}
pub(crate) fn finish_retirement(s: &Scheduler, owner: CellOwner) -> Result<bool> {
    mutate(s, |a| {
        let slot = owner_index(owner)?;
        if a.header.state == 0 || a.groups[slot].state == 0 { return Ok(true) }
        a.advance()?;
        let i = a.group_index(owner.cell_id, owner.generation, owner.root_tid)?;
        if a.occupied(owner.cell_id, owner.generation) { return Ok(false) }
        if a.groups[i].state != RETIRING { return Err(Error::RetirementPending) }
        if a.retired_count >= N as u64 { return Err(Error::HistoryFull) }
        a.reserve_events(1)?;
        let mut row = a.groups[i];
        row.state = RETIRED;
        row.terminal_serial = a.events[..a.header.event_count as usize].iter().rev()
            .find(|e| e.kind == 4 && e.cell_id == owner.cell_id && e.generation == owner.generation)
            .map_or(0, |e| e.value1);
        a.append_reserved(Event { now_ns: a.header.last_ns, kind: 10, cell_id: owner.cell_id,
            generation: owner.generation, hart: u64::MAX, value0: owner.root_tid,
            value1: a.retired_count, ..Event::ZERO });
        a.retired[a.retired_count as usize] = row;
        a.retired_count += 1;
        a.groups[i] = Group::ZERO;
        Ok(true)
    })
}
#[allow(dead_code)]
pub(crate) fn request_policy(s: &Scheduler, owner: CellOwner, revision: u64, e: u64, b: u64) -> Result<()> {
    mutate(s, |a| {
        a.advance()?;
        let i = a.group_index(owner.cell_id, owner.generation, owner.root_tid)?;
        let g = a.groups[i];
        if revision != g.policy_revision.checked_add(1).ok_or(Error::CounterExhausted)?
            || g.pending_revision != 0 || (g.state != LIVE && g.state != THROTTLED) { return Err(Error::PolicyConflict) }
        if e == 0 || e > mul(a.header.period_ns, a.header.admitted_hart_mask.count_ones() as u64)? { return Err(Error::BadConfig) }
        add(e, b)?;
        a.emit(8, a.header.last_ns, owner.cell_id, owner.generation, u64::MAX, e, b)?;
        a.groups[i].pending_revision = revision;
        a.groups[i].pending_entitlement_ns = e; a.groups[i].pending_burst_cap_ns = b;
        a.groups[i].pending_apply_period = add(a.header.period_index, 1)?;
        Ok(())
    })
}

fn line<const W: usize>(tag: &str, index: usize, words: [u64; W]) {
    // Fixed largest row (24 words), formatted outside both source locks.
    let mut hex = [0u8; 384];
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for (i, word) in words.iter().enumerate() {
        for (j, byte) in word.to_le_bytes().iter().enumerate() {
            hex[(i*8+j)*2] = DIGITS[(byte >> 4) as usize];
            hex[(i*8+j)*2+1] = DIGITS[(byte & 15) as usize];
        }
    }
    let text = core::str::from_utf8(&hex[..W*16]).expect("hex ASCII");
    log::warn!("{} index={} hex={}", tag, index, text);
}
/// Invoke outside all source locks. Only the first coherent settled terminal
/// cutoff exports; copying individual frozen rows never holds locks while logging.
pub(crate) fn safe_point() {
    let header = {
        let scheduler = super::SCHEDULER.lock();
        let Some(s) = scheduler.as_ref() else { return };
        let mut a = ACCOUNTING.lock();
        if a.header.state != 1 || a.retired_count == 0 || a.groups.iter().any(|g| g.state != 0)
            || a.harts.iter().any(|h| h.pending_serial != 0 || h.active_cell != 0) { return }
        if let Err(e) = a.advance() {
            a.seal(e); drop(a); drop(scheduler); fatal(e)
        }
        let now = a.header.last_ns;
        let retired = a.retired_count;
        if let Err(e) = a.emit(12, now, 0, 0, u64::MAX, retired, 0) {
            a.seal(e); drop(a); drop(scheduler); fatal(e)
        }
        let _ = s;
        a.header.state = 3;
        a.header
    };
    line("G1-HEADER", 0, header.words());
    let mut manifest_hex = [0u8; 192];
    for (i, b) in crate::p01_measurement::MANIFEST.iter().enumerate() {
        manifest_hex[i*2] = b"0123456789abcdef"[(b >> 4) as usize];
        manifest_hex[i*2+1] = b"0123456789abcdef"[(b & 15) as usize];
    }
    log::warn!("G1-MANIFEST hex={}", core::str::from_utf8(&manifest_hex).expect("hex ASCII"));
    for i in 0..M { let row = { let _s = super::SCHEDULER.lock(); ACCOUNTING.lock().harts[i] }; line("G1-HART", i, row.words()); }
    for i in 0..N {
        let (group, retired) = { let _s = super::SCHEDULER.lock(); let a = ACCOUNTING.lock(); (a.groups[i], a.retired[i]) };
        if group.state != 0 { line("G1-GROUP", i, group.words()); }
        if retired.state != 0 { line("G1-RETIRED", i, retired.words()); }
    }
    for i in 0..header.event_count as usize {
        let row = { let _s = super::SCHEDULER.lock(); ACCOUNTING.lock().events[i] };
        line("G1-EVENT", i, row.words());
    }
    let c = match crate::hal::hpet::checked_sample() {
        Ok(c) => c,
        Err(_) => {
            { let _s = super::SCHEDULER.lock(); ACCOUNTING.lock().seal(Error::ClockUnavailable); }
            fatal(Error::ClockUnavailable)
        }
    };
    let final_identity = {
        let _s = super::SCHEDULER.lock();
        let mut a = ACCOUNTING.lock();
        let result = a.check_clock_identity(&c).and_then(|_| {
            if c.raw_counter < a.header.last_raw_counter { Err(Error::ClockRegression) }
            else { Ok(()) }
        });
        if let Err(e) = result { a.seal(e); }
        result
    };
    require(final_identity);
    log::warn!("G1-CLOCK raw={} period_fs={} base={} capabilities={} configuration={}",
        c.raw_counter, c.period_fs, c.base, c.capabilities, c.configuration);
    let retired = { let _s = super::SCHEDULER.lock(); ACCOUNTING.lock().retired_count };
    log::warn!("G1-CUTOFF events={} retired={} pending=0 state={} error={} payload=41304",
        header.event_count, retired, header.state, header.error);
    unsafe { core::arch::asm!("out dx, eax", in("dx") 0xf4u16, in("eax") 0x10u32); }
    loop { unsafe { core::arch::asm!("cli", "hlt"); } }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Accounting {
        let mut a = Accounting { groups: [Group::ZERO; N], harts: [Hart::ZERO; M],
            header: Header::ZERO, events: [Event::ZERO; EVENTS],
            retired: [Group::ZERO; N], retired_count: 0 };
        a.header = Header { state: 1, period_ns: 100, next_boundary_ns: 100,
            admitted_hart_mask: 3, exhaustion_to_departure_limit_ns: 20, ..Header::ZERO };
        a.groups[1] = Group { cell_id: 1, generation: 7, root_tid: 11, state: LIVE,
            entitlement_ns: 20, burst_cap_ns: 40, policy_revision: 1, ..Group::ZERO };
        a
    }
    #[test]
    fn parallel_occupancy_and_outgoing_pending_are_not_double_counted() {
        let mut a = model();
        for (i, h) in a.harts.iter_mut().enumerate() {
            *h = Hart { active_cell: 1, active_generation: 7, active_root_tid: 11,
                active_tid: 11+i as u64, pending_cell: 1, pending_generation: 7,
                pending_root_tid: 11, pending_tid: 13+i as u64, pending_serial: 1+i as u64,
                ..Hart::ZERO };
        }
        a.segment(5).unwrap();
        assert_eq!(a.groups[1].period_charge_ns, 10);
        assert_eq!(a.header.control_charge_ns, 0);
    }
    #[test]
    fn first_period_prorates_and_settlement_repays_tail_debt() {
        let mut a = model();
        a.groups[1].birth_ns = 75;
        assert_eq!(a.allowance(&a.groups[1]).unwrap(), 5);
        a.groups[1].period_charge_ns = 8;
        a.settle().unwrap();
        assert_eq!(a.groups[1].debt_ns, 3);
        assert_eq!(a.groups[1].burst_bank_ns, 0);
        assert_eq!(a.allowance(&a.groups[1]).unwrap(), 17);
    }
    #[test]
    fn successor_policy_applies_only_after_old_charge_settles() {
        let mut a = model();
        a.groups[1].period_charge_ns = 25;
        a.groups[1].pending_revision = 2;
        a.groups[1].pending_entitlement_ns = 10;
        a.groups[1].pending_burst_cap_ns = 0;
        a.groups[1].pending_apply_period = 1;
        a.settle().unwrap();
        assert_eq!(a.groups[1].debt_ns, 5);
        assert_eq!(a.groups[1].entitlement_ns, 10);
        assert_eq!(a.groups[1].last_period_allowance_ns, 20);
        assert_eq!(a.allowance(&a.groups[1]).unwrap(), 5);
    }
    #[test]
    fn real_departure_deadline_survives_replenishment() {
        let mut a = model();
        a.harts[0] = Hart { active_cell: 1, active_generation: 7,
            active_root_tid: 11, active_tid: 11, ..Hart::ZERO };
        a.segment(20).unwrap();
        assert_eq!(a.groups[1].terminal_serial, 21);
        a.settle().unwrap();
        assert_eq!(a.check_departure(41), Err(Error::OvershootExceeded));
    }
    #[test]
    fn full_witness_retains_prior_events_and_fixed_failure() {
        let mut a = model();
        for i in 0..EVENTS {
            a.emit(5, i as u64, 1, 7, 0, 11, 1).unwrap();
        }
        assert_eq!(a.emit(5, 999, 1, 7, 0, 11, 1), Err(Error::WitnessFull));
        a.seal(Error::WitnessFull);
        assert_eq!(a.events[0].now_ns, 0);
        assert_eq!(a.events[EVENTS-1].now_ns, 255);
        assert_eq!(a.header.error, Error::WitnessFull as u64);
        assert_eq!(a.header.event_count, 256);
    }
    #[test]
    fn malformed_or_stale_owner_never_indexes_or_aliases_storage() {
        let a = model();
        assert_eq!(owner_index(CellOwner::new(u64::MAX, 7, 11)), Err(Error::UnknownOwner));
        assert_eq!(a.group_index(1, 8, 11), Err(Error::StaleGeneration));
        assert!(a.group_index(1, 7, 11).is_ok());
    }
    #[test]
    fn segment_capacity_failure_preserves_confirmed_interval() {
        let mut a = model();
        a.harts[0] = Hart { active_cell: 1, active_generation: 7,
            active_root_tid: 11, active_tid: 11, ..Hart::ZERO };
        a.header.event_count = EVENTS as u64;
        assert_eq!(a.segment(20), Err(Error::WitnessFull));
        assert_eq!(a.header.last_ns, 0);
        assert_eq!(a.header.control_charge_ns, 0);
        assert_eq!(a.groups[1].period_charge_ns, 0);
        assert_eq!(a.groups[1].lifetime_charge_ns, 0);
        assert_eq!(a.groups[1].terminal_serial, 0);
        assert_eq!(a.groups[1].state, LIVE);
    }
    #[test]
    fn period_preflight_rejects_without_settling_earlier_rows() {
        let mut a = model();
        a.groups[1].period_charge_ns = 5;
        a.groups[2] = Group { cell_id: 2, generation: 8, root_tid: 22,
            state: LIVE, entitlement_ns: u64::MAX, burst_bank_ns: 1, ..Group::ZERO };
        assert_eq!(a.settle(), Err(Error::ArithmeticOverflow));
        assert_eq!(a.header.period_index, 0);
        assert_eq!(a.header.next_boundary_ns, 100);
        assert_eq!(a.header.event_count, 0);
        assert_eq!(a.groups[1].period_charge_ns, 5);
        assert_eq!(a.groups[1].burst_bank_ns, 0);
    }
    #[test]
    fn service_summary_retains_maximum_and_checked_continuations() {
        let mut a = model();
        a.header.service_gap_limit_ns = 20;
        a.harts[0] = Hart { active_cell: 1, active_generation: 7,
            active_root_tid: 11, active_tid: 11, ..Hart::ZERO };
        a.header.last_ns = 5;
        a.service_summary(0, true, false).unwrap();
        a.harts[0].last_service_ns = 5;
        for now in 6..50 {
            a.header.last_ns = now;
            a.service_summary(0, true, false).unwrap();
            a.harts[0].last_service_ns = now;
        }
        assert_eq!(a.header.event_count, 1);
        assert_eq!(a.events[0].kind, 17);
        assert_eq!(a.events[0].value0, 5);
        assert_eq!(a.events[0].value1, 45);
        assert_eq!(a.events[0].now_ns, 49);
    }
}

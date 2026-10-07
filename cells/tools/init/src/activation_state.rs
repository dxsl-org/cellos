//! Bounded demand ownership, independent of the scheduler and IPC transport.
use ocel_service_proto::{Engine, Failure, MAX_LEASES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Principal {
    pub cell_id: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub principal: Principal,
    pub root_tid: usize,
    pub watch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase { Starting, Ready, Retiring }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Provider {
    pub engine: Engine,
    pub tid: usize,
    pub generation: u64,
    pub phase: Phase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lease {
    pub owner: Owner,
    pub engine: Engine,
    pub token: u64,
    pub provider_generation: u64,
}

pub(crate) struct Effects {
    pub cancel: [Option<u64>; MAX_LEASES],
    pub retire: [Option<Provider>; 2],
}

impl Effects {
    fn new() -> Self { Self { cancel: [None; MAX_LEASES], retire: [None; 2] } }
    fn cancel(&mut self, token: u64) {
        if let Some(slot) = self.cancel.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(token);
        }
    }
}

pub(crate) struct State {
    leases: [Option<Lease>; MAX_LEASES],
    providers: [Option<Provider>; 2],
    next_token: u64,
    next_generation: u64,
}

fn index(engine: Engine) -> usize { engine as usize - 1 }

impl State {
    pub const fn new() -> Self {
        Self { leases: [None; MAX_LEASES], providers: [None; 2], next_token: 1, next_generation: 1 }
    }

    pub fn provider(&self, engine: Engine) -> Option<Provider> { self.providers[index(engine)] }

    pub fn owner(&self, principal: Principal) -> Option<Owner> {
        self.leases.iter().flatten().find(|lease| lease.owner.principal == principal).map(|lease| lease.owner)
    }

    pub fn lease(&self, principal: Principal, engine: Engine) -> Option<Lease> {
        self.leases.iter().flatten().find(|lease| lease.owner.principal == principal && lease.engine == engine).copied()
    }

    /// Duplicate demand succeeds even at capacity; starting/retiring instances
    /// are never published or reused. Counter exhaustion never reuses a token.
    pub fn preflight(&self, principal: Principal, engine: Engine) -> Result<(), Failure> {
        if self.provider(engine).is_some_and(|provider| provider.phase != Phase::Ready) {
            return Err(Failure::Busy);
        }
        if self.lease(principal, engine).is_some() { return Ok(()); }
        if self.next_token == 0 || self.leases.iter().all(Option::is_some) { return Err(Failure::Busy); }
        if self.provider(engine).is_none() && self.next_generation == 0 { return Err(Failure::Busy); }
        Ok(())
    }

    pub fn start(&mut self, engine: Engine, tid: usize) -> Result<Provider, Failure> {
        if tid == 0 || self.provider(engine).is_some() || self.next_generation == 0 { return Err(Failure::Busy); }
        let provider = Provider { engine, tid, generation: self.next_generation, phase: Phase::Starting };
        self.next_generation = self.next_generation.checked_add(1).unwrap_or(0);
        self.providers[index(engine)] = Some(provider);
        Ok(provider)
    }

    pub fn ready(&mut self, provider: Provider) -> bool {
        let slot = &mut self.providers[index(provider.engine)];
        if *slot != Some(provider) || provider.phase != Phase::Starting { return false; }
        *slot = Some(Provider { phase: Phase::Ready, ..provider });
        true
    }

    /// The bool is allocation provenance, used to roll back undelivered Ready
    /// replies without revoking a previously delivered idempotent acquisition.
    pub fn acquire(&mut self, owner: Owner, engine: Engine) -> Result<(Lease, bool), Failure> {
        self.preflight(owner.principal, engine)?;
        let provider = self.provider(engine).filter(|provider| provider.phase == Phase::Ready).ok_or(Failure::Failed)?;
        if let Some(lease) = self.lease(owner.principal, engine) { return Ok((lease, false)); }
        if self.owner(owner.principal).is_some_and(|existing| existing != owner) { return Err(Failure::Denied); }
        let slot = self.leases.iter_mut().find(|slot| slot.is_none()).ok_or(Failure::Busy)?;
        let lease = Lease { owner, engine, token: self.next_token, provider_generation: provider.generation };
        self.next_token = self.next_token.checked_add(1).unwrap_or(0);
        *slot = Some(lease);
        Ok((lease, true))
    }

    pub fn release(&mut self, principal: Principal, engine: Engine, token: u64) -> Result<Effects, Failure> {
        let provider = self.provider(engine).ok_or(Failure::Denied)?;
        let slot = self.leases.iter().position(|slot| slot.is_some_and(|lease|
            lease.owner.principal == principal && lease.engine == engine && lease.token == token
                && lease.provider_generation == provider.generation)).ok_or(Failure::Denied)?;
        let mut effects = Effects::new();
        self.remove(slot, &mut effects);
        self.retire_unused(&mut effects);
        Ok(effects)
    }

    fn remove(&mut self, slot: usize, effects: &mut Effects) {
        if let Some(lease) = self.leases[slot].take() {
            if self.owner(lease.owner.principal).is_none() { effects.cancel(lease.owner.watch); }
        }
    }

    fn retire_unused(&mut self, effects: &mut Effects) {
        for engine in [Engine::JavaScript, Engine::Pdf] {
            if self.leases.iter().flatten().any(|lease| lease.engine == engine) { continue; }
            if let Some(provider) = self.provider(engine) {
                let retiring = Provider { phase: Phase::Retiring, ..provider };
                self.providers[index(engine)] = Some(retiring);
                effects.retire[index(engine)] = Some(retiring);
            }
        }
    }

    pub fn unused(&mut self) -> Effects {
        let mut effects = Effects::new();
        self.retire_unused(&mut effects);
        effects
    }

    pub fn owner_dead(&mut self, root_tid: usize) -> Effects {
        let mut effects = Effects::new();
        for slot in 0..MAX_LEASES {
            if self.leases[slot].is_some_and(|lease| lease.owner.root_tid == root_tid) { self.remove(slot, &mut effects); }
        }
        self.retire_unused(&mut effects);
        effects
    }

    pub fn watches_root(&self, root_tid: usize) -> bool {
        self.leases.iter().flatten().any(|lease| lease.owner.root_tid == root_tid)
    }

    /// Revoke every stale token before attempting termination. A failed kill
    /// leaves a quarantined Retiring instance, not an acquirable stale service.
    pub fn retire(&mut self, engine: Engine) -> Effects {
        let mut effects = Effects::new();
        for slot in 0..MAX_LEASES {
            if self.leases[slot].is_some_and(|lease| lease.engine == engine) { self.remove(slot, &mut effects); }
        }
        self.retire_unused(&mut effects);
        effects
    }

    pub fn terminated(&mut self, provider: Provider) {
        if self.provider(provider.engine) == Some(provider) { self.providers[index(provider.engine)] = None; }
    }

    pub fn engine_dead(&mut self, tid: usize) -> Option<Effects> {
        let provider = self.providers.iter().flatten().find(|provider| provider.tid == tid).copied()?;
        let mut effects = Effects::new();
        for slot in 0..MAX_LEASES {
            if self.leases[slot].is_some_and(|lease| lease.engine == provider.engine) { self.remove(slot, &mut effects); }
        }
        self.providers[index(provider.engine)] = None;
        Some(effects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner(id: u64) -> Owner { Owner { principal: Principal { cell_id: id, generation: 1 }, root_tid: id as usize + 100, watch: id + 1000 } }
    fn ready(state: &mut State, engine: Engine, tid: usize) {
        let provider = state.start(engine, tid).unwrap();
        assert!(state.ready(provider));
    }

    #[test]
    fn sharing_duplicate_and_final_release() {
        let mut state = State::new();
        ready(&mut state, Engine::JavaScript, 10);
        let (a, new) = state.acquire(owner(1), Engine::JavaScript).unwrap();
        assert!(new);
        assert_eq!(state.acquire(owner(1), Engine::JavaScript), Ok((a, false)));
        let (b, _) = state.acquire(owner(2), Engine::JavaScript).unwrap();
        let effects = state.release(a.owner.principal, a.engine, a.token).unwrap();
        assert!(effects.retire.iter().all(Option::is_none));
        assert_eq!(effects.cancel[0], Some(a.owner.watch));
        let effects = state.release(b.owner.principal, b.engine, b.token).unwrap();
        let retiring = effects.retire[0].unwrap();
        assert_eq!(state.preflight(owner(3).principal, a.engine), Err(Failure::Busy));
        state.terminated(retiring);
        assert_eq!(state.provider(a.engine), None);
    }

    #[test]
    fn stale_release_cannot_affect_replacement_or_foreign_owner() {
        let mut state = State::new();
        ready(&mut state, Engine::Pdf, 10);
        let (old, _) = state.acquire(owner(1), Engine::Pdf).unwrap();
        let effects = state.release(old.owner.principal, old.engine, old.token).unwrap();
        state.terminated(effects.retire[1].unwrap());
        ready(&mut state, Engine::Pdf, 11);
        let (new, _) = state.acquire(owner(1), Engine::Pdf).unwrap();
        assert_ne!(old.token, new.token);
        assert!(state.release(old.owner.principal, old.engine, old.token).is_err());
        assert!(state.release(owner(2).principal, new.engine, new.token).is_err());
        assert_eq!(state.lease(new.owner.principal, new.engine), Some(new));
    }

    #[test]
    fn capacity_keeps_duplicate_demand_and_reclaims_slots() {
        let mut state = State::new();
        ready(&mut state, Engine::Pdf, 10);
        for id in 1..=MAX_LEASES as u64 { state.acquire(owner(id), Engine::Pdf).unwrap(); }
        assert_eq!(state.preflight(owner(99).principal, Engine::Pdf), Err(Failure::Busy));
        assert!(state.acquire(owner(1), Engine::Pdf).is_ok());
        let effects = state.owner_dead(owner(1).root_tid);
        assert_eq!(effects.cancel[0], Some(owner(1).watch));
        assert!(state.acquire(owner(99), Engine::Pdf).is_ok());
    }

    #[test]
    fn crash_revokes_all_shared_tokens_without_restart() {
        let mut state = State::new();
        ready(&mut state, Engine::JavaScript, 10);
        let (a, _) = state.acquire(owner(1), Engine::JavaScript).unwrap();
        state.acquire(owner(2), Engine::JavaScript).unwrap();
        let effects = state.engine_dead(10).unwrap();
        assert_eq!(effects.cancel.iter().flatten().count(), 2);
        assert!(effects.retire.iter().all(Option::is_none));
        assert_eq!(state.provider(a.engine), None);
        assert!(state.release(a.owner.principal, a.engine, a.token).is_err());
        ready(&mut state, a.engine, 11);
        let (b, _) = state.acquire(a.owner, a.engine).unwrap();
        assert_ne!(a.token, b.token);
        assert!(state.engine_dead(10).is_none());
    }

    #[test]
    fn root_watch_lasts_across_both_kinds_and_death_stops_both() {
        let mut state = State::new();
        ready(&mut state, Engine::JavaScript, 10);
        ready(&mut state, Engine::Pdf, 11);
        let (js, _) = state.acquire(owner(1), Engine::JavaScript).unwrap();
        state.acquire(owner(1), Engine::Pdf).unwrap();
        let effects = state.release(js.owner.principal, js.engine, js.token).unwrap();
        assert!(effects.cancel.iter().all(Option::is_none));
        state.terminated(effects.retire[0].unwrap());
        let effects = state.owner_dead(owner(1).root_tid);
        assert_eq!(effects.cancel.iter().flatten().count(), 1);
        assert_eq!(effects.retire[1].unwrap().tid, 11);
        assert!(!state.watches_root(owner(1).root_tid));
    }

    #[test]
    fn failed_reply_rollback_preserves_other_demand_and_watch() {
        let mut state = State::new();
        ready(&mut state, Engine::Pdf, 10);
        let (a, _) = state.acquire(owner(1), Engine::Pdf).unwrap();
        let (b, new) = state.acquire(owner(2), Engine::Pdf).unwrap();
        assert!(new);
        let effects = state.release(b.owner.principal, b.engine, b.token).unwrap();
        assert!(effects.retire.iter().all(Option::is_none));
        assert_eq!(state.lease(a.owner.principal, a.engine), Some(a));
        assert_eq!(state.acquire(a.owner, a.engine), Ok((a, false)));
    }

    #[test]
    fn owner_generation_is_not_a_user_supplied_cell_alias() {
        let mut state = State::new();
        ready(&mut state, Engine::Pdf, 10);
        let (old, _) = state.acquire(owner(1), Engine::Pdf).unwrap();
        let new_owner = Owner {
            principal: Principal { generation: 2, ..old.owner.principal },
            root_tid: 999,
            watch: 2001,
        };
        let (new, _) = state.acquire(new_owner, Engine::Pdf).unwrap();
        assert_ne!(old.token, new.token);
        assert!(state.release(new_owner.principal, old.engine, old.token).is_err());
        let effects = state.owner_dead(old.owner.root_tid);
        assert!(effects.retire.iter().all(Option::is_none));
        assert_eq!(state.lease(new_owner.principal, Engine::Pdf), Some(new));
    }

    #[test]
    fn retirement_revokes_tokens_before_kill_and_old_completion_is_inert() {
        let mut state = State::new();
        ready(&mut state, Engine::Pdf, 10);
        let (lease, _) = state.acquire(owner(1), Engine::Pdf).unwrap();
        let effects = state.retire(Engine::Pdf);
        let retired = effects.retire[1].unwrap();
        assert!(state.release(lease.owner.principal, lease.engine, lease.token).is_err());
        assert_eq!(state.preflight(owner(1).principal, Engine::Pdf), Err(Failure::Busy));
        assert_eq!(effects.cancel[0], Some(lease.owner.watch));
        state.terminated(retired);
        ready(&mut state, Engine::Pdf, 11);
        state.terminated(retired);
        assert_eq!(state.provider(Engine::Pdf).unwrap().tid, 11);
    }

    #[test]
    fn failed_start_has_no_published_lease_and_can_be_retired() {
        let mut state = State::new();
        let starting = state.start(Engine::Pdf, 10).unwrap();
        assert_eq!(state.acquire(owner(1), Engine::Pdf), Err(Failure::Busy));
        let effects = state.unused();
        let retiring = effects.retire[1].unwrap();
        assert_eq!(retiring.phase, Phase::Retiring);
        assert!(!state.ready(starting));
        assert!(effects.cancel.iter().all(Option::is_none));
        state.terminated(retiring);
        assert_eq!(state.provider(Engine::Pdf), None);
    }

    #[test]
    fn exhaustion_never_wraps_tokens_or_provider_generations() {
        let mut state = State::new();
        ready(&mut state, Engine::Pdf, 10);
        state.next_token = u64::MAX;
        let (lease, _) = state.acquire(owner(1), Engine::Pdf).unwrap();
        assert_eq!(lease.token, u64::MAX);
        assert_eq!(state.preflight(owner(2).principal, Engine::Pdf), Err(Failure::Busy));
        assert!(state.acquire(owner(1), Engine::Pdf).is_ok());
        state.engine_dead(10).unwrap();
        state.next_generation = 0;
        assert_eq!(state.start(Engine::Pdf, 11), Err(Failure::Busy));
    }
}

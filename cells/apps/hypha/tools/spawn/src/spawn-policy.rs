//! What `tool-spawn` may start and stop on behalf of its `core`.
//!
//! WHY: `tool-spawn` holds SpawnCap and acts on LLM output that crossed the
//! network. Unconstrained, `spawn_cell` reaches every `/bin` cell the
//! tool-spawn launch edge admits (network clients and listeners included) and
//! `kill_cell` reaches every non-system task. Both are narrowed to what an
//! assistant needs: start a reviewed, capability-free app, and stop only what it
//! started itself.

/// Exact paths `spawn_cell` may launch. Matching is byte-exact: no prefixes, no
/// normalisation, so `..`, doubled slashes and other absolute paths can never
/// match. Each entry must declare no VFS-mutate, network, device or spawn
/// authority, and must run without argv (tool-spawn passes none).
pub const SPAWN_ALLOWLIST: &[&str] = &[
    // Plays a fixed tone sequence and exits; declares only `Log` + `AudioPlay`.
    "/bin/audio-demo",
    // Self-contained game; manifest is block_io/network/spawn = false.
    "/bin/tetris",
];

/// Most cells one session may have alive through `spawn_cell` at once. Bounds
/// the tracking table and how many cells LLM output can keep resident.
pub const MAX_SPAWNED: usize = 8;

/// Whether `spawn_cell` may launch `path`.
pub fn spawn_allowed(path: &str) -> bool {
    SPAWN_ALLOWLIST.contains(&path)
}

/// Tids this `tool-spawn` instance started and has not yet killed.
///
/// Invariant: `kill_cell` acts only on a tid in this set. Tids are never
/// re-issued within one boot, so an entry can never come to name a cell this
/// session did not start.
pub struct SpawnedCells {
    tids: [usize; MAX_SPAWNED],
    len: usize,
}

impl SpawnedCells {
    pub const fn new() -> Self {
        Self {
            tids: [0; MAX_SPAWNED],
            len: 0,
        }
    }

    /// No room for another spawn until one tracked cell is killed.
    pub fn is_full(&self) -> bool {
        self.len == MAX_SPAWNED
    }

    /// Track a freshly spawned `tid`. Returns `false` (and tracks nothing) when
    /// full or for tid 0, which never names a task.
    pub fn record(&mut self, tid: usize) -> bool {
        if tid == 0 || self.is_full() || self.contains(tid) {
            return false;
        }
        self.tids[self.len] = tid;
        self.len += 1;
        true
    }

    pub fn contains(&self, tid: usize) -> bool {
        tid != 0 && self.tids[..self.len].contains(&tid)
    }

    /// Stop tracking `tid`; `false` when it was not tracked.
    pub fn release(&mut self, tid: usize) -> bool {
        let Some(at) = self.tids[..self.len].iter().position(|t| *t == tid) else {
            return false;
        };
        self.len -= 1;
        self.tids[at] = self.tids[self.len];
        self.tids[self.len] = 0;
        true
    }
}

impl Default for SpawnedCells {
    fn default() -> Self {
        Self::new()
    }
}

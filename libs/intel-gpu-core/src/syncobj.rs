//! DRM sync objects as the `xe` uAPI exposes them to Mesa (`include/uapi/drm/drm.h`
//! and `drivers/gpu/drm/drm_syncobj.c`, both MIT): binary payloads, timelines of
//! points, `WAIT_FOR_SUBMIT`, `WAIT_AVAILABLE`, `WAIT_ALL`, and `QUERY`.
//!
//! One [`Syncobjs`] is one client session's handle namespace. Waiting is split:
//! [`Syncobjs::poll_wait`] decides now; the driver Cell re-polls on fence
//! completion and answers `ETIME` itself when the caller's deadline passes.
//! A timeline point is complete only when it and every earlier point are
//! (`dma_fence_chain` order). Fences that completed with an error count as
//! signalled, as `dma_fence_is_signaled` does.

use alloc::vec::Vec;

pub const CREATE_SIGNALED: u32 = 1 << 0;
pub const WAIT_ALL: u32 = 1 << 0;
pub const WAIT_FOR_SUBMIT: u32 = 1 << 1;
pub const WAIT_AVAILABLE: u32 = 1 << 2;
pub const WAIT_DEADLINE: u32 = 1 << 3;
pub const QUERY_LAST_SUBMITTED: u32 = 1 << 0;

/// Errors with the errno `drm_syncobj.c` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncError {
    /// `-EINVAL`: bad flags, empty query, or a missing fence without `WAIT_FOR_SUBMIT`.
    Invalid,
    /// `-ENOENT`: unknown handle.
    NoEntry,
}

/// A GPU (or CPU-created) fence owned by the driver Cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FenceId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FenceState {
    Pending,
    Done,
}

#[derive(Clone, Copy)]
struct Node {
    point: u64,
    fence: FenceId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitState {
    /// Satisfied; `first` is the `first_signaled` index (0 under `WAIT_ALL`).
    Ready { first: u32 },
    /// Not yet; re-poll after a fence completes or a point is added.
    Pending,
}

#[derive(Default)]
pub struct Syncobjs {
    /// Index = handle - 1. A binary payload is a single node with point 0.
    objects: Vec<Option<Vec<Node>>>,
    fences: Vec<FenceState>,
}

impl Syncobjs {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new fence for submitted GPU work; complete it with [`Self::signal_fence`].
    pub fn new_fence(&mut self) -> FenceId {
        self.fences.push(FenceState::Pending);
        FenceId(self.fences.len() as u32 - 1)
    }

    fn done_fence(&mut self) -> FenceId {
        let id = self.new_fence();
        self.signal_fence(id);
        id
    }

    /// GPU completion (success or error) of `fence`.
    pub fn signal_fence(&mut self, fence: FenceId) {
        self.fences[fence.0 as usize] = FenceState::Done;
    }

    fn done(&self, node: &Node) -> bool {
        self.fences[node.fence.0 as usize] == FenceState::Done
    }

    pub fn create(&mut self, flags: u32) -> Result<u32, SyncError> {
        if flags & !CREATE_SIGNALED != 0 {
            return Err(SyncError::Invalid);
        }
        let payload = if flags & CREATE_SIGNALED != 0 {
            let fence = self.done_fence();
            alloc::vec![Node { point: 0, fence }]
        } else {
            Vec::new()
        };
        self.objects.push(Some(payload));
        Ok(self.objects.len() as u32)
    }

    fn get_mut(&mut self, handle: u32) -> Result<&mut Vec<Node>, SyncError> {
        let slot = handle.checked_sub(1).ok_or(SyncError::NoEntry)? as usize;
        self.objects
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or(SyncError::NoEntry)
    }

    fn get(&self, handle: u32) -> Result<&Vec<Node>, SyncError> {
        let slot = handle.checked_sub(1).ok_or(SyncError::NoEntry)? as usize;
        self.objects
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or(SyncError::NoEntry)
    }

    pub fn destroy(&mut self, handle: u32) -> Result<(), SyncError> {
        self.get(handle)?;
        self.objects[handle as usize - 1] = None;
        Ok(())
    }

    /// `SYNCOBJ_RESET`: drop the payload. All handles are checked before any change.
    pub fn reset(&mut self, handles: &[u32]) -> Result<(), SyncError> {
        handles.iter().try_for_each(|&h| self.get(h).map(drop))?;
        handles
            .iter()
            .for_each(|&h| self.objects[h as usize - 1] = Some(Vec::new()));
        Ok(())
    }

    /// Attach `fence` at `point` (0 = replace the payload, the binary behaviour).
    /// Used for `EXEC`/`VM_BIND` signal syncs and CPU signals.
    pub fn attach(&mut self, handle: u32, point: u64, fence: FenceId) -> Result<(), SyncError> {
        let nodes = self.get_mut(handle)?;
        if point == 0 {
            nodes.clear();
        }
        nodes.push(Node { point, fence });
        Ok(())
    }

    /// `SYNCOBJ_SIGNAL` (points all 0) and `SYNCOBJ_TIMELINE_SIGNAL`.
    pub fn signal(&mut self, handles: &[u32], points: &[u64]) -> Result<(), SyncError> {
        if handles.len() != points.len() {
            return Err(SyncError::Invalid);
        }
        handles.iter().try_for_each(|&h| self.get(h).map(drop))?;
        for (&h, &p) in handles.iter().zip(points) {
            let fence = self.done_fence();
            self.attach(h, p, fence)?;
        }
        Ok(())
    }

    /// Highest point such that it and all earlier points are complete (0 if none),
    /// or with [`QUERY_LAST_SUBMITTED`] the last point added. Binary payloads read 0.
    pub fn query(&self, handle: u32, flags: u32) -> Result<u64, SyncError> {
        if flags & !QUERY_LAST_SUBMITTED != 0 {
            return Err(SyncError::Invalid);
        }
        let nodes = self.get(handle)?;
        if flags & QUERY_LAST_SUBMITTED != 0 {
            return Ok(nodes.last().map_or(0, |n| n.point));
        }
        Ok(nodes
            .iter()
            .take_while(|n| self.done(n))
            .last()
            .map_or(0, |n| n.point))
    }

    /// Fence state for `point`: `None` when no fence covers it yet; `Some(done)` otherwise.
    fn point_state(&self, nodes: &[Node], point: u64) -> Option<bool> {
        let upto = if point == 0 {
            nodes.len().checked_sub(1)?
        } else {
            nodes.iter().position(|n| n.point >= point)?
        };
        Some(nodes[..=upto].iter().all(|n| self.done(n)))
    }

    /// One evaluation of `SYNCOBJ_WAIT` (`timeline = false`, points ignored) or
    /// `SYNCOBJ_TIMELINE_WAIT`. An empty handle list is ready, as upstream.
    pub fn poll_wait(
        &self,
        handles: &[u32],
        points: &[u64],
        flags: u32,
        timeline: bool,
    ) -> Result<WaitState, SyncError> {
        let allowed =
            WAIT_ALL | WAIT_FOR_SUBMIT | WAIT_DEADLINE | if timeline { WAIT_AVAILABLE } else { 0 };
        if flags & !allowed != 0 || (timeline && handles.len() != points.len()) {
            return Err(SyncError::Invalid);
        }
        let lenient = flags & (WAIT_FOR_SUBMIT | WAIT_AVAILABLE) != 0;
        let mut ready = 0u32;
        let mut first = None;
        for (i, &h) in handles.iter().enumerate() {
            let point = if timeline { points[i] } else { 0 };
            let ok = match self.point_state(self.get(h)?, point) {
                None if lenient => false,
                None => return Err(SyncError::Invalid),
                Some(done) => done || flags & WAIT_AVAILABLE != 0,
            };
            if ok {
                ready += 1;
                first.get_or_insert(i as u32);
            }
        }
        let all = flags & WAIT_ALL != 0;
        Ok(match first {
            _ if ready as usize == handles.len() => WaitState::Ready {
                first: if all { 0 } else { first.unwrap_or(0) },
            },
            Some(first) if !all => WaitState::Ready { first },
            _ => WaitState::Pending,
        })
    }
}

#[cfg(test)]
mod tests;

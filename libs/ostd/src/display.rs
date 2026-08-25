//! App-side display helpers: `ViSurface` for Grant-backed compositor surfaces.
//!
//! ## Usage
//! ```
//! let comp_tid = wait_for_compositor();
//! let mut surf = ViSurface::create(comp_tid, 640, 480, PixelFormat::Bgra8888)?;
//! let px = surf.pixels_mut();
//! // draw into px ...
//! surf.damage_all();
//! ```

extern crate alloc;

use api::display::{
    compositor_events, compositor_ops, AttachGrant, DamageNotify, PixelFormat, Rect, SetTitle,
    SurfaceRole, SurfaceStateRequest, WindowCloseRequest, WindowConfigure, WindowStateChanged,
};
use api::syscall::service;
use types::{ViError, ViResult};

use crate::syscall::{
    sys_grant_register, sys_grant_share, sys_grant_slice, sys_grant_unregister, sys_lookup_service,
    sys_recv, sys_send, SyscallResult,
};

// ─── Service lookup ───────────────────────────────────────────────────────────

/// Block until the compositor service is registered and return its TID.
pub fn wait_for_compositor() -> usize {
    loop {
        if let Some(tid) = sys_lookup_service(service::COMPOSITOR) {
            return tid;
        }
        crate::task::yield_now();
    }
}

/// A typed lifecycle event sent by the trusted compositor to a surface owner.
///
/// These events are retained in a bounded, allocation-free dispatcher until
/// [`poll_surface_events`] returns them.
#[repr(C, u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceEvent {
    /// The compositor proposes a new content rectangle.
    Configure(WindowConfigure),
    /// The compositor asks the owner to accept or reject a close.
    CloseRequest(WindowCloseRequest),
    /// The compositor reports a managed-state transition.
    StateChanged(WindowStateChanged),
}

/// Maximum number of surface capabilities retained by the lifecycle dispatcher.
pub const MAX_SURFACE_EVENT_CAPS: usize = 32;

/// Maximum lifecycle events that can be retained at once.
pub const MAX_SURFACE_EVENTS: usize = MAX_SURFACE_EVENT_CAPS * 3;

const MAX_FORWARDED_INPUT_EVENTS: usize = 32;

#[derive(Clone, Copy)]
struct Stamped<T: Copy> {
    sequence: u64,
    value: T,
}

#[derive(Clone, Copy)]
struct SurfaceEventSlot {
    cap: u32,
    used: bool,
    configure: Option<Stamped<WindowConfigure>>,
    close: Option<Stamped<WindowCloseRequest>>,
    state: Option<Stamped<WindowStateChanged>>,
}

impl SurfaceEventSlot {
    const EMPTY: Self = Self {
        cap: 0,
        used: false,
        configure: None,
        close: None,
        state: None,
    };
}

struct SurfaceEventDispatcher {
    next_sequence: u64,
    slots: [SurfaceEventSlot; MAX_SURFACE_EVENT_CAPS],
    forwarded_input: heapless::Vec<api::input::InputEvent, MAX_FORWARDED_INPUT_EVENTS>,
}

impl SurfaceEventDispatcher {
    const fn new() -> Self {
        Self {
            next_sequence: 1,
            slots: [SurfaceEventSlot::EMPTY; MAX_SURFACE_EVENT_CAPS],
            forwarded_input: heapless::Vec::new(),
        }
    }

    fn sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        sequence
    }

    fn slot_for(&mut self, cap: u32) -> Option<&mut SurfaceEventSlot> {
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.used && slot.cap == cap)
        {
            return Some(&mut self.slots[index]);
        }
        let index = self.slots.iter().position(|slot| !slot.used)?;
        self.slots[index] = SurfaceEventSlot {
            cap,
            used: true,
            ..SurfaceEventSlot::EMPTY
        };
        Some(&mut self.slots[index])
    }

    fn insert(&mut self, event: SurfaceEvent) {
        let cap = match event {
            SurfaceEvent::Configure(frame) => frame.cap,
            SurfaceEvent::CloseRequest(frame) => frame.cap,
            SurfaceEvent::StateChanged(frame) => frame.cap,
        };
        let sequence = self.sequence();
        let Some(slot) = self.slot_for(cap) else {
            return;
        };
        match event {
            SurfaceEvent::Configure(frame) => {
                slot.configure = Some(Stamped {
                    sequence,
                    value: frame,
                });
            }
            SurfaceEvent::CloseRequest(frame) => {
                slot.close = Some(Stamped {
                    sequence,
                    value: frame,
                });
            }
            SurfaceEvent::StateChanged(frame) => {
                slot.state = Some(Stamped {
                    sequence,
                    value: frame,
                });
            }
        }
    }
    fn take_surface(&mut self) -> Option<SurfaceEvent> {
        let mut selected: Option<(usize, u8, u64)> = None;
        for (index, slot) in self.slots.iter().enumerate() {
            for (kind, sequence) in [
                (0, slot.configure.map(|event| event.sequence)),
                (1, slot.close.map(|event| event.sequence)),
                (2, slot.state.map(|event| event.sequence)),
            ] {
                if let Some(sequence) = sequence {
                    let replace = match selected {
                        Some((_, _, oldest)) => sequence < oldest,
                        None => true,
                    };
                    if replace {
                        selected = Some((index, kind, sequence));
                    }
                }
            }
        }
        let (index, kind, _) = selected?;
        let slot = &mut self.slots[index];
        let event = match kind {
            0 => slot
                .configure
                .take()
                .map(|event| SurfaceEvent::Configure(event.value)),
            1 => slot
                .close
                .take()
                .map(|event| SurfaceEvent::CloseRequest(event.value)),
            _ => slot
                .state
                .take()
                .map(|event| SurfaceEvent::StateChanged(event.value)),
        };
        if slot.configure.is_none() && slot.close.is_none() && slot.state.is_none() {
            *slot = SurfaceEventSlot::EMPTY;
        }
        event
    }
}

static SURFACE_EVENTS: crate::sync::Mutex<SurfaceEventDispatcher> =
    crate::sync::Mutex::new(SurfaceEventDispatcher::new());

/// Poll up to `max` retained compositor lifecycle events.
///
/// # Returns
/// A fixed-capacity vector containing at most `min(max, MAX_SURFACE_EVENTS)`
/// events, ordered by receipt after state coalescing.
///
/// The result is a fixed-capacity vector, so polling does not allocate. Only
/// frames received from the currently registered compositor service are routed;
/// input frames it forwards remain available to [`crate::input::poll_events`].
pub fn poll_surface_events(max: usize) -> heapless::Vec<SurfaceEvent, MAX_SURFACE_EVENTS> {
    let limit = max.min(MAX_SURFACE_EVENTS);
    let mut events = heapless::Vec::new();
    while events.len() < limit {
        let Some(event) = SURFACE_EVENTS.lock().take_surface() else {
            break;
        };
        let _ = events.push(event);
    }

    let Some(compositor_tid) = sys_lookup_service(service::COMPOSITOR) else {
        return events;
    };
    for _ in 0..limit.saturating_sub(events.len()) {
        if !drain_compositor_once(compositor_tid) {
            break;
        }
        while events.len() < limit {
            let Some(event) = SURFACE_EVENTS.lock().take_surface() else {
                break;
            };
            let _ = events.push(event);
        }
    }
    events
}

/// Receive and route one frame from `compositor_tid`.
///
/// The source mask is kernel-enforced, so a frame from an unrelated sender is
/// never consumed by this dispatcher.
pub(crate) fn drain_compositor_once(compositor_tid: usize) -> bool {
    let mut buffer = [0u8; 72];
    match crate::syscall::sys_try_recv(compositor_tid, &mut buffer) {
        SyscallResult::Ok(sender) if sender == compositor_tid => {
            route_compositor_frame(&buffer);
            true
        }
        _ => false,
    }
}

/// Route one trusted compositor frame without allocating.
pub(crate) fn route_compositor_frame(frame: &[u8]) {
    let Some(opcode) = frame.first().copied() else {
        return;
    };
    let mut dispatcher = SURFACE_EVENTS.lock();
    match opcode {
        api::input::INPUT_EVENT_OPCODE => {
            if let Some(event) = crate::input::parse_frame(frame) {
                let _ = dispatcher.forwarded_input.push(event);
            }
        }
        compositor_events::WINDOW_CONFIGURE => {
            if let Ok(event) = WindowConfigure::decode(frame.get(..28).unwrap_or_default()) {
                dispatcher.insert(SurfaceEvent::Configure(event));
            }
        }
        compositor_events::WINDOW_CLOSE_REQUEST => {
            if let Ok(event) = WindowCloseRequest::decode(frame.get(..12).unwrap_or_default()) {
                dispatcher.insert(SurfaceEvent::CloseRequest(event));
            }
        }
        compositor_events::WINDOW_STATE_CHANGED => {
            if let Ok(event) = WindowStateChanged::decode(frame.get(..12).unwrap_or_default()) {
                dispatcher.insert(SurfaceEvent::StateChanged(event));
            }
        }
        _ => {}
    }
}

/// Take one compositor-forwarded input event retained by the shared dispatcher.
pub(crate) fn take_forwarded_input_event() -> Option<api::input::InputEvent> {
    let mut dispatcher = SURFACE_EVENTS.lock();
    if dispatcher.forwarded_input.is_empty() {
        None
    } else {
        Some(dispatcher.forwarded_input.remove(0))
    }
}

// ─── ViSurface ────────────────────────────────────────────────────────────────

/// A compositor surface backed by a Grant buffer the app cell owns directly.
///
/// The app writes pixels into `pixels_mut()` and calls `damage()` / `damage_all()`
/// to tell the compositor which regions need to be re-blended.  No pixel data
/// crosses an IPC boundary — only a 24-byte `DamageNotify` is sent per dirty region.
///
/// ## Lifecycle
/// `ViSurface::create` → write pixels → `damage` → … → `drop` (auto-cleans up).
///
/// `ViSurface` is `!Send`: the Grant pointer must stay on the cell's task.
pub struct ViSurface {
    comp_tid: usize,
    cap: u32,
    reg_id: usize,
    retired_reg_id: Option<usize>,
    staged_reg_id: Option<usize>,
    ptr: *mut u8,
    width: u32,
    height: u32,
    fmt: PixelFormat,
    /// Makes `ViSurface` !Send on stable Rust — the raw pointer must stay on its origin task.
    _not_send: core::marker::PhantomData<*mut ()>,
}

impl ViSurface {
    /// Create an interactive surface of `(width × height)` pixels.
    ///
    /// Allocates a persistent Grant buffer, shares it read-only with the compositor,
    /// and sends `CREATE_SURFACE` + `ATTACH_GRANT` IPC.
    ///
    /// # Errors
    /// - `OutOfMemory` if `sys_grant_register` fails.
    /// - `IO` if the compositor rejects `ATTACH_GRANT` (e.g. too many surfaces).
    pub fn create(comp_tid: usize, width: u32, height: u32, fmt: PixelFormat) -> ViResult<Self> {
        Self::create_with_role(comp_tid, width, height, fmt, SurfaceRole::Interactive)
    }

    /// Create a visible surface that cannot receive desktop pointer or keyboard focus.
    pub fn create_background(
        comp_tid: usize,
        width: u32,
        height: u32,
        fmt: PixelFormat,
    ) -> ViResult<Self> {
        Self::create_with_role(comp_tid, width, height, fmt, SurfaceRole::Background)
    }

    fn create_with_role(
        comp_tid: usize,
        width: u32,
        height: u32,
        fmt: PixelFormat,
        role: SurfaceRole,
    ) -> ViResult<Self> {
        let size = (width * height * fmt.bpp()) as usize;

        // 1. Allocate a persistent physical Grant buffer (lives until we call unregister).
        let reg_id = sys_grant_register(size).ok_or(ViError::OutOfMemory)?;

        // 2. Share read-only with compositor so it can read our pixels.
        sys_grant_share(reg_id, comp_tid, 0 /* ReadOnly */);

        // 3. Get our own write pointer into the Grant.
        let ptr = sys_grant_slice(reg_id).ok_or_else(|| {
            sys_grant_unregister(reg_id);
            ViError::IO
        })?;

        // 4. Ask compositor to create a surface slot → get cap.
        let cap = ipc_create_surface(comp_tid, width, height, role).inspect_err(|_e| {
            sys_grant_unregister(reg_id);
        })?;

        // 5. Tell compositor to attach our Grant to that slot.
        ipc_attach_grant(comp_tid, cap, reg_id, width, height, fmt).inspect_err(|_e| {
            let _ = ipc_destroy_surface(comp_tid, cap);
            sys_grant_unregister(reg_id);
        })?;

        Ok(Self {
            comp_tid,
            cap,
            reg_id,
            retired_reg_id: None,
            staged_reg_id: None,
            ptr,
            width,
            height,
            fmt,
            _not_send: core::marker::PhantomData,
        })
    }

    /// Direct mutable access to the pixel buffer.
    ///
    /// The app writes directly here; the compositor reads it via a read-only Grant.
    /// After writing, call `damage()` or `damage_all()` to trigger a repaint.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        let len = (self.width * self.height * self.fmt.bpp()) as usize;
        // SAFETY: ptr is our own registered Grant buffer (sys_grant_register).
        // We hold &mut self so no other code can call pixels_mut concurrently.
        // The compositor is expected to treat this Grant as read-only, but in
        // today's SAS build `GrantPerm` is only a protocol contract, not Tier 2
        // hardware isolation against a malicious compositor.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, len) }
    }

    /// Stride in bytes (width × bytes-per-pixel).
    pub fn stride(&self) -> usize {
        self.width as usize * self.fmt.bpp() as usize
    }

    /// Surface width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Surface height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Surface capability carried by compositor lifecycle events for this surface.
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// Signal a dirty region to the compositor (fire-and-forget, 24-byte IPC).
    ///
    /// The compositor will re-blend this rect on the next render tick.
    pub fn damage(&self, rect: Rect) {
        let msg = DamageNotify {
            opcode: compositor_ops::DAMAGE_NOTIFY,
            _pad: [0; 3],
            cap: self.cap,
            rect,
        };
        sys_send(self.comp_tid, &msg.encode());
    }

    /// Signal the entire surface as dirty.
    pub fn damage_all(&self) {
        self.damage(Rect {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        });
    }

    /// Move this surface to a new screen position.
    pub fn move_to(&self, x: i32, y: i32) {
        let mut buf = [0u8; 13];
        buf[0] = compositor_ops::MOVE_SURFACE;
        buf[1..9].copy_from_slice(&(self.cap as u64).to_le_bytes());
        buf[9..13].copy_from_slice(&x.to_le_bytes());
        // y needs 4 more bytes — extend to 17
        let mut buf17 = [0u8; 17];
        buf17[..13].copy_from_slice(&buf);
        buf17[13..17].copy_from_slice(&y.to_le_bytes());
        sys_send(self.comp_tid, &buf17);
    }

    /// Raise this surface to the top of the z-order.
    pub fn raise(&self) {
        let mut buf = [0u8; 9];
        buf[0] = compositor_ops::RAISE_SURFACE;
        buf[1..9].copy_from_slice(&(self.cap as u64).to_le_bytes());
        sys_send(self.comp_tid, &buf);
    }
    /// Replace this surface's UTF-8 `title`.
    ///
    /// The request is fire-and-forget; the compositor may reject it according
    /// to its ownership policy.
    ///
    /// # Errors
    /// Returns `InvalidInput` when `title` exceeds 64 UTF-8 bytes, or `IO` when
    /// the request cannot be sent to the compositor.
    pub fn set_title(&self, title: &str) -> ViResult<()> {
        let request = SetTitle::new(self.cap, title).map_err(|_| ViError::InvalidInput)?;
        let frame = request.encode().map_err(|_| ViError::InvalidInput)?;
        send_lifecycle_request(self.comp_tid, &frame)
    }

    /// Request that this surface be minimized.
    ///
    /// # Errors
    /// Returns `IO` when the request cannot be sent to the compositor.
    pub fn minimize(&self) -> ViResult<()> {
        self.request_state(compositor_ops::MINIMIZE)
    }

    /// Request that this surface be maximized.
    ///
    /// # Errors
    /// Returns `IO` when the request cannot be sent to the compositor.
    pub fn maximize(&self) -> ViResult<()> {
        self.request_state(compositor_ops::MAXIMIZE)
    }

    /// Request that this surface be restored from a managed state.
    ///
    /// # Errors
    /// Returns `IO` when the request cannot be sent to the compositor.
    pub fn restore(&self) -> ViResult<()> {
        self.request_state(compositor_ops::RESTORE)
    }

    /// Acknowledge the compositor configuration `serial` for this surface.
    ///
    /// This only transmits the acknowledgement. Use [`Self::apply_configure`]
    /// when the proposal changes dimensions and therefore requires a staged
    /// replacement Grant.
    ///
    /// # Errors
    /// Returns `IO` when the acknowledgement cannot be sent to the compositor.
    pub fn acknowledge_configure(&self, serial: u32) -> ViResult<()> {
        ipc_configure_ack(self.comp_tid, self.cap, serial)
    }

    /// A new read-only Grant is allocated and attached before the matching
    /// configure acknowledgement is sent. Local pixels, dimensions, and Grant
    /// registration change only after both compositor replies succeed. The old
    /// Grant remains registered until `DETACH_REPLACED_GRANT` confirms that the
    /// compositor has released it; `Drop` releases it if that cleanup is deferred.
    /// # Errors
    /// Returns `InvalidArgument` when `configure` names another surface, has a
    /// zero serial, a zero dimension, or an overflowing pixel size;
    /// `OutOfMemory` when registration fails; and `IO` for Grant mapping or
    /// compositor protocol failures. Before the configure acknowledgement, all
    /// errors preserve this surface's existing Grant and dimensions.
    pub fn apply_configure(&mut self, configure: WindowConfigure) -> ViResult<()> {
        if configure.cap != self.cap
            || configure.serial == 0
            || configure.rect.w == 0
            || configure.rect.h == 0
            || self.staged_reg_id.is_some()
            || self.retired_reg_id.is_some()
        {
            return Err(ViError::InvalidArgument);
        }
        let new_size = surface_byte_len(configure.rect.w, configure.rect.h, self.fmt)?;
        let new_reg_id = sys_grant_register(new_size).ok_or(ViError::OutOfMemory)?;
        sys_grant_share(new_reg_id, self.comp_tid, 0 /* ReadOnly */);
        let new_ptr = match sys_grant_slice(new_reg_id) {
            Some(pointer) => pointer,
            None => {
                sys_grant_unregister(new_reg_id);
                return Err(ViError::IO);
            }
        };
        // An IPC error is ambiguous; retain this mapping until the compositor
        // explicitly confirms that it has detached the staged Grant.
        self.staged_reg_id = Some(new_reg_id);
        match ipc_stage_grant(
            self.comp_tid,
            self.cap,
            new_reg_id,
            configure.rect.w,
            configure.rect.h,
            self.fmt,
        ) {
            AttachGrantResult::Attached => {}
            AttachGrantResult::Rejected => {
                self.staged_reg_id = None;
                sys_grant_unregister(new_reg_id);
                return Err(ViError::IO);
            }
            AttachGrantResult::AmbiguousFailure => return Err(ViError::IO),
        }
        // The acknowledged attachment remains staged until its configure ACK.
        if ipc_configure_ack(self.comp_tid, self.cap, configure.serial).is_err() {
            if ipc_detach_grant(self.comp_tid, self.cap).is_ok() {
                self.staged_reg_id = None;
                sys_grant_unregister(new_reg_id);
            }
            return Err(ViError::IO);
        }

        let old_reg_id = self.reg_id;
        self.reg_id = new_reg_id;
        self.retired_reg_id = Some(old_reg_id);
        self.ptr = new_ptr;
        self.staged_reg_id = None;
        self.width = configure.rect.w;
        self.height = configure.rect.h;
        self.detach_retired_grant();
        Ok(())
    }

    /// Respond to the compositor close-request `serial` for this surface.
    ///
    /// `accept` names whether the owner accepts (`true`) or rejects (`false`)
    /// the request. A stale or forged serial is rejected by the compositor.
    ///
    /// # Errors
    /// Returns `IO` when the response cannot be sent to the compositor.
    pub fn respond_close(&self, serial: u32, accept: bool) -> ViResult<()> {
        let frame = api::display::CloseResponse::new(self.cap, serial, accept)
            .encode()
            .map_err(|_| ViError::InvalidInput)?;
        send_lifecycle_request(self.comp_tid, &frame)
    }

    fn request_state(&self, opcode: u8) -> ViResult<()> {
        let request =
            SurfaceStateRequest::new(self.cap, opcode).map_err(|_| ViError::InvalidInput)?;
        let frame = request.encode().map_err(|_| ViError::InvalidInput)?;
        send_lifecycle_request(self.comp_tid, &frame)
    }

    fn detach_retired_grant(&mut self) {
        let Some(reg_id) = self.retired_reg_id else {
            return;
        };
        if ipc_detach_replaced_grant(self.comp_tid, self.cap, reg_id).is_ok() {
            sys_grant_unregister(reg_id);
            self.retired_reg_id = None;
        }
    }

    /// Explicitly destroy the surface (also called by `Drop`).
    pub fn destroy(self) {
        drop(self);
    }
}

impl Drop for ViSurface {
    fn drop(&mut self) {
        // 1. Detach grant — compositor stops reading from our buffer.
        let mut detach = [0u8; 9];
        detach[0] = compositor_ops::DETACH_GRANT;
        detach[1..9].copy_from_slice(&(self.cap as u64).to_le_bytes());
        sys_send(self.comp_tid, &detach);
        // Drain the reply while preserving any intervening lifecycle event.
        let _ = ipc_receive_status(self.comp_tid, 0x01);

        // 2. Destroy surface slot in compositor.
        let _ = ipc_destroy_surface(self.comp_tid, self.cap);

        // 3. Release physical Grant pages after compositor ownership has ended.
        sys_grant_unregister(self.reg_id);
        if let Some(reg_id) = self.staged_reg_id {
            sys_grant_unregister(reg_id);
        }
        if let Some(reg_id) = self.retired_reg_id {
            sys_grant_unregister(reg_id);
        }
    }
}

/// Send a lifecycle request without waiting for a compositor implementation reply.
fn send_lifecycle_request(comp_tid: usize, frame: &[u8]) -> ViResult<()> {
    match sys_send(comp_tid, frame) {
        SyscallResult::Ok(_) => Ok(()),
        SyscallResult::Err(_) => Err(ViError::IO),
    }
}
// ─── Private IPC helpers ──────────────────────────────────────────────────────

fn ipc_create_surface(comp_tid: usize, w: u32, h: u32, role: SurfaceRole) -> ViResult<u32> {
    let mut req = [0u8; 10];
    req[0] = compositor_ops::CREATE_SURFACE;
    req[1..5].copy_from_slice(&w.to_le_bytes());
    req[5..9].copy_from_slice(&h.to_le_bytes());
    req[9] = role as u8;
    sys_send(comp_tid, &req);

    let mut resp = [0u8; 8];
    match sys_recv(comp_tid, &mut resp) {
        SyscallResult::Ok(sender) if sender == comp_tid => {
            let cap = u32::from_le_bytes([resp[0], resp[1], resp[2], resp[3]]);
            if cap == 0 {
                Err(ViError::IO)
            } else {
                Ok(cap)
            }
        }
        _ => Err(ViError::IO),
    }
}

fn surface_byte_len(width: u32, height: u32, fmt: PixelFormat) -> ViResult<usize> {
    let pixels = width.checked_mul(height).ok_or(ViError::InvalidArgument)?;
    let bytes = pixels
        .checked_mul(fmt.bpp())
        .ok_or(ViError::InvalidArgument)?;
    Ok(bytes as usize)
}

/// Receive the expected compositor status without losing lifecycle or input frames.
///
/// Frames are routed immediately rather than queued here, so a burst of
/// interleaved compositor events cannot turn an already-committed transaction
/// into an ambiguous failure. Rejected transactions terminate with `0x00`;
/// successful ones with `0x01`.
fn ipc_receive_status(comp_tid: usize, expected_status: u8) -> ViResult<()> {
    loop {
        let mut frame = [0u8; 72];
        match sys_recv(comp_tid, &mut frame) {
            SyscallResult::Ok(sender) if sender == comp_tid => match frame[0] {
                api::input::INPUT_EVENT_OPCODE
                | compositor_events::WINDOW_CONFIGURE
                | compositor_events::WINDOW_CLOSE_REQUEST
                | compositor_events::WINDOW_STATE_CHANGED => {
                    route_compositor_frame(&frame);
                }
                status if status == expected_status => return Ok(()),
                _ => return Err(ViError::IO),
            },
            _ => return Err(ViError::IO),
        }
    }
}

/// Send `CONFIGURE_ACK` and require the compositor's success reply.
fn ipc_configure_ack(comp_tid: usize, cap: u32, serial: u32) -> ViResult<()> {
    let ack = api::display::ConfigureAck::new(cap, serial)
        .encode()
        .map_err(|_| ViError::InvalidInput)?;
    sys_send(comp_tid, &ack);

    ipc_receive_status(comp_tid, 0x01)
}

fn ipc_detach_grant(comp_tid: usize, cap: u32) -> ViResult<()> {
    let mut request = [0u8; 9];
    request[0] = compositor_ops::DETACH_GRANT;
    request[1..].copy_from_slice(&(cap as u64).to_le_bytes());
    sys_send(comp_tid, &request);
    ipc_receive_status(comp_tid, 0x01)
}

/// Release only the retired Grant after a successful replacement commit.
fn ipc_detach_replaced_grant(comp_tid: usize, cap: u32, old_reg_id: usize) -> ViResult<()> {
    let request = api::display::DetachReplacedGrant::new(cap, old_reg_id as u64)
        .encode()
        .map_err(|_| ViError::InvalidInput)?;
    sys_send(comp_tid, &request);

    ipc_receive_status(comp_tid, 0x01)
}

enum AttachGrantResult {
    Attached,
    Rejected,
    AmbiguousFailure,
}

/// Send `ATTACH_GRANT` and distinguish an explicit rejection from transport loss.
fn ipc_stage_grant(
    comp_tid: usize,
    cap: u32,
    reg_id: usize,
    w: u32,
    h: u32,
    fmt: PixelFormat,
) -> AttachGrantResult {
    let ag = AttachGrant {
        opcode: compositor_ops::ATTACH_GRANT,
        fmt: fmt as u8,
        _pad: [0; 2],
        cap,
        reg_id: reg_id as u64,
        width: w,
        height: h,
    };
    if !matches!(sys_send(comp_tid, &ag.encode()), SyscallResult::Ok(_)) {
        return AttachGrantResult::AmbiguousFailure;
    }
    loop {
        let mut frame = [0u8; 72];
        match sys_recv(comp_tid, &mut frame) {
            SyscallResult::Ok(sender) if sender == comp_tid => match frame[0] {
                api::input::INPUT_EVENT_OPCODE
                | compositor_events::WINDOW_CONFIGURE
                | compositor_events::WINDOW_CLOSE_REQUEST
                | compositor_events::WINDOW_STATE_CHANGED => route_compositor_frame(&frame),
                0x01 => return AttachGrantResult::Attached,
                0x00 => return AttachGrantResult::Rejected,
                _ => return AttachGrantResult::AmbiguousFailure,
            },
            _ => return AttachGrantResult::AmbiguousFailure,
        }
    }
}

fn ipc_attach_grant(
    comp_tid: usize,
    cap: u32,
    reg_id: usize,
    w: u32,
    h: u32,
    fmt: PixelFormat,
) -> ViResult<()> {
    match ipc_stage_grant(comp_tid, cap, reg_id, w, h, fmt) {
        AttachGrantResult::Attached => Ok(()),
        AttachGrantResult::Rejected | AttachGrantResult::AmbiguousFailure => Err(ViError::IO),
    }
}

/// Send `DESTROY_SURFACE` (best-effort; errors silently ignored on Drop path).
fn ipc_destroy_surface(comp_tid: usize, cap: u32) -> ViResult<()> {
    let mut req = [0u8; 9];
    req[0] = compositor_ops::DESTROY_SURFACE;
    req[1..9].copy_from_slice(&(cap as u64).to_le_bytes());
    sys_send(comp_tid, &req);

    ipc_receive_status(comp_tid, 0x00)
}

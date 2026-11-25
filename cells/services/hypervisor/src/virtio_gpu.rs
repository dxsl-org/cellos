#![allow(dead_code)]
//! VirtIO-GPU 2D device model (DeviceID=16, MMIO slot 3, SPI 19).

mod client_pointer;
mod client_scanout;
mod command;
mod dispatch;
mod resource;
mod rules;
mod scanout;
mod wire;

use crate::virtio_mmio::{QueueCfg, VirtioDevice};
use crate::virtqueue::process_notify;
use client_scanout::ClientScanout;
use resource::ResourceTable;
use scanout::ScanoutBridge;

/// `virtio_gpu_config.events_read` bit: the display configuration changed.
const EVENT_DISPLAY: u32 = 1;

/// Where the guest scanout is shown: a full-screen compositor surface (console
/// and lane use, `hv`), or the window of the launcher bound to the session.
pub enum Presenter {
    Compositor(ScanoutBridge),
    Client(ClientScanout),
}

impl Presenter {
    fn notify_damage(&mut self, resources: &ResourceTable, rect: command::Rect) {
        match self {
            Self::Compositor(bridge) => bridge.notify_damage(rect),
            Self::Client(client) => client.notify_damage(resources, rect),
        }
    }

    /// Whether the guest's cursor plane is drawn into the scanout. A launcher's
    /// window lies under the host pointer, so there the guest's cursor commands
    /// are consumed and nothing is drawn: one pointer, and no scanout copy per
    /// pointer move.
    fn draws_guest_cursor(&self) -> bool {
        matches!(self, Self::Compositor(_))
    }
}

#[cfg(target_arch = "aarch64")]
const GPU_SPI: u32 = 19;

pub struct GpuDev {
    resources: ResourceTable,
    presenter: Presenter,
    width: u32,
    height: u32,
    events_read: u32,
    last_avail: [u16; 2],
    used_idx: [u16; 2],
}

/// Display width offered to a launcher's guest for a window `width` wide.
///
/// Linux's virtio-gpu builds its only mode with `drm_cvt_mode`, which rounds the
/// width down to 8 pixels, and a scanout resource must match the offered size
/// exactly (`ResourceTable::set_scanout`). A maximized window offered 1278 got a
/// 1272 resource back, the scanout was refused and the window stayed black.
pub fn guest_display_width(width: u32) -> u32 {
    width & !7
}

impl GpuDev {
    /// A device shown on a full-screen compositor surface.
    pub fn new(compositor_tid: usize, width: u32, height: u32) -> Self {
        Self::with_presenter(
            Presenter::Compositor(ScanoutBridge::new(compositor_tid, width, height)),
            width,
            height,
        )
    }

    /// A device shown in the window of the launcher `client_tid`.
    pub fn for_client(client_tid: usize, width: u32, height: u32) -> Self {
        Self::with_presenter(
            Presenter::Client(ClientScanout::new(client_tid, width, height)),
            width,
            height,
        )
    }

    fn with_presenter(presenter: Presenter, width: u32, height: u32) -> Self {
        Self {
            resources: ResourceTable::new(),
            presenter,
            width,
            height,
            events_read: 0,
            last_avail: [0; 2],
            used_idx: [0; 2],
        }
    }

    pub fn bring_up(&mut self) {
        match &mut self.presenter {
            Presenter::Compositor(bridge) => bridge.bring_up(&mut self.resources),
            Presenter::Client(client) => client.bring_up(&mut self.resources),
        }
    }

    pub fn poll_damage(&mut self) {
        match &mut self.presenter {
            Presenter::Compositor(bridge) => bridge.poll_damage(),
            Presenter::Client(client) => client.poll(&self.resources),
        }
    }

    pub fn reconnect_compositor(&mut self, compositor_tid: usize) {
        if let Presenter::Compositor(bridge) = &mut self.presenter {
            bridge.reconnect(compositor_tid, &mut self.resources);
        }
    }

    /// Stop presenting; a bound launcher is told the guest stopped.
    pub fn shutdown(&mut self) {
        match &mut self.presenter {
            Presenter::Compositor(bridge) => bridge.reset(&mut self.resources),
            Presenter::Client(client) => {
                client.exited(0);
                self.resources.teardown_scanout();
            }
        }
    }

    /// Change the display size the guest is offered (launcher window resize).
    /// Returns `true` when the guest must be told through a config-change
    /// interrupt; it re-reads the display info and sets a new scanout.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        if (width, height) == (self.width, self.height) {
            return false;
        }
        let Presenter::Client(client) = &mut self.presenter else {
            return false;
        };
        self.width = width;
        self.height = height;
        client.resize(&mut self.resources, width, height);
        self.events_read |= EVENT_DISPLAY;
        true
    }

    /// Scanout dimensions the guest framebuffer is presented at.
    pub fn scanout_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl VirtioDevice for GpuDev {
    fn device_id(&self) -> u32 {
        16
    }

    fn device_features_lo(&self) -> u32 {
        0
    }

    fn config_read(&self, offset: usize) -> u32 {
        match offset {
            0 => self.events_read,
            8 => 1,
            _ => 0,
        }
    }

    fn config_write(&mut self, offset: usize, value: u32) {
        // events_clear: the guest acknowledges handled events.
        if offset == 4 {
            self.events_read &= !value;
        }
    }
    fn notify(&mut self, queue: usize, qcfg: &QueueCfg, vm_id: usize, vcpu_id: usize) -> bool {
        let published = match queue {
            0 => process_notify(
                vm_id,
                qcfg,
                &mut self.last_avail[0],
                &mut self.used_idx[0],
                |bufs| {
                    dispatch::handle_control(
                        &mut self.resources,
                        &mut self.presenter,
                        bufs,
                        vm_id,
                        self.width,
                        self.height,
                    )
                },
            ),
            1 => {
                let presenter = &mut self.presenter;
                let resources = &mut self.resources;
                process_notify(
                    vm_id,
                    qcfg,
                    &mut self.last_avail[1],
                    &mut self.used_idx[1],
                    |bufs| match presenter {
                        Presenter::Compositor(_) => dispatch::handle_cursor(resources, bufs, vm_id),
                        Presenter::Client(client) => {
                            dispatch::handle_client_cursor(client, resources, bufs, vm_id)
                        }
                    },
                )
            }
            _ => return false,
        };
        if published == 0 {
            return false;
        }
        if queue == 1 && self.presenter.draws_guest_cursor() {
            if let Some((_, width, height, _)) = self.resources.bound_resource() {
                self.presenter.notify_damage(
                    &self.resources,
                    command::Rect {
                        x: 0,
                        y: 0,
                        width,
                        height,
                    },
                );
            }
        }
        // aarch64 raises the SPI here; x86 delivers the ISR status that
        // `notify`'s `true` sets through the emulated PIC (x86_irq_dispatch).
        #[cfg(target_arch = "aarch64")]
        crate::vmm::inject_irq(vm_id, vcpu_id, crate::vmm::device_irq(GPU_SPI));
        #[cfg(not(target_arch = "aarch64"))]
        let _ = vcpu_id;
        true
    }

    fn reset(&mut self) {
        match &mut self.presenter {
            Presenter::Compositor(bridge) => bridge.reset(&mut self.resources),
            // A driver reset is not the guest stopping: the launcher keeps its
            // Grant, and the guest sets its scanout again.
            Presenter::Client(_) => self.resources.reset(),
        }
        self.last_avail = [0; 2];
        self.used_idx = [0; 2];
    }
}

/// `rect` clipped to a `width`x`height` scanout; `None` if nothing remains.
fn clip_rect(rect: command::Rect, width: u32, height: u32) -> Option<command::Rect> {
    let clipped = command::Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width.min(width.saturating_sub(rect.x)),
        height: rect.height.min(height.saturating_sub(rect.y)),
    };
    (clipped.width != 0 && clipped.height != 0).then_some(clipped)
}

/// The smallest rectangle covering `a` and `b`.
fn union_rect(a: command::Rect, b: command::Rect) -> command::Rect {
    let left = a.x.min(b.x);
    let top = a.y.min(b.y);
    let right = a.x.saturating_add(a.width).max(b.x.saturating_add(b.width));
    let bottom =
        a.y.saturating_add(a.height)
            .max(b.y.saturating_add(b.height));
    command::Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    }
}

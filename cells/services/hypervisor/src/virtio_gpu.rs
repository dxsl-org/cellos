//! VirtIO-GPU 2D device model (DeviceID=16, MMIO slot 3, SPI 19).

mod command;
mod dispatch;
mod resource;
mod rules;
mod scanout;
mod wire;

use crate::virtio_mmio::{QueueCfg, VirtioDevice};
use crate::virtqueue::process_notify;
use resource::ResourceTable;
use scanout::ScanoutBridge;

const GPU_SPI: u32 = 19;

pub struct GpuDev {
    resources: ResourceTable,
    scanout: ScanoutBridge,
    width: u32,
    height: u32,
    last_avail: [u16; 2],
    used_idx: [u16; 2],
}

impl GpuDev {
    pub fn new(compositor_tid: usize, width: u32, height: u32) -> Self {
        Self {
            resources: ResourceTable::new(),
            scanout: ScanoutBridge::new(compositor_tid, width, height),
            width,
            height,
            last_avail: [0; 2],
            used_idx: [0; 2],
        }
    }

    pub fn bring_up(&mut self) {
        self.scanout.bring_up(&mut self.resources);
    }

    pub fn poll_damage(&mut self) {
        self.scanout.poll_damage();
    }

    pub fn reconnect_compositor(&mut self, compositor_tid: usize) {
        self.scanout.reconnect(compositor_tid, &mut self.resources);
    }

    pub fn shutdown(&mut self) {
        self.scanout.reset(&mut self.resources);
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
            8 => 1,
            _ => 0,
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
                        &mut self.scanout,
                        bufs,
                        vm_id,
                        self.width,
                        self.height,
                    )
                },
            ),
            1 => process_notify(
                vm_id,
                qcfg,
                &mut self.last_avail[1],
                &mut self.used_idx[1],
                |bufs| dispatch::handle_cursor(&mut self.resources, bufs, vm_id),
            ),
            _ => return false,
        };
        if published == 0 {
            return false;
        }
        if queue == 1 {
            if let Some((_, width, height, _)) = self.resources.bound_resource() {
                self.scanout.notify_damage(command::Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                });
            }
        }
        crate::vmm::inject_irq(vm_id, vcpu_id, GPU_SPI);
        true
    }

    fn reset(&mut self) {
        self.scanout.reset(&mut self.resources);
        self.last_avail = [0; 2];
        self.used_idx = [0; 2];
    }
}

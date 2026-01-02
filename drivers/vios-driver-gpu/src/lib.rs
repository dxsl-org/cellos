#![no_std]

use vios_driver_virtio::{VirtioMmio, Virtqueue, VRING_DESC_F_NEXT, VRING_DESC_F_WRITE};

pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
pub const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
pub const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
pub const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;

pub const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
pub const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct GpuHeader {
    pub ctrl_type: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub padding: u32,
}

#[repr(C)]
pub struct ResourceCreate2d {
    pub hdr: GpuHeader,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
pub struct TransferToHost2d {
    pub hdr: GpuHeader,
    pub x: u32, pub y: u32, pub width: u32, pub height: u32,
    pub offset: u64,
    pub resource_id: u32,
    pub padding: u32,
}

#[repr(C)]
pub struct ResourceAttachBackingContiguous {
    pub hdr: GpuHeader,
    pub resource_id: u32,
    pub nr_entries: u32,
    pub addr: u64,
    pub length: u32,
    pub padding: u32,
}

#[repr(C)]
pub struct SetScanout {
    pub hdr: GpuHeader,
    pub x: u32, pub y: u32, pub width: u32, pub height: u32,
    pub scanout_id: u32,
    pub resource_id: u32,
}

#[repr(C)]
pub struct ResourceFlush {
    pub hdr: GpuHeader,
    pub x: u32, pub y: u32, pub width: u32, pub height: u32,
    pub resource_id: u32,
    pub padding: u32,
}

pub struct VirtioGpu {
    mmio: VirtioMmio,
}

impl VirtioGpu {
    pub unsafe fn new(base: usize) -> Self {
        Self { mmio: VirtioMmio::new(base) }
    }

    pub unsafe fn init_device(&self, q: &mut Virtqueue) {
        if !self.mmio.init(16) { return; }
        self.mmio.setup_queue(0, q as *const _ as usize);
        self.mmio.complete_init();
    }

    pub unsafe fn resource_create_2d(&self, q: &mut Virtqueue, resource_id: u32, width: u32, height: u32, resp_addr: u64) -> u32 {
        let mut cmd = ResourceCreate2d {
            hdr: GpuHeader { ctrl_type: VIRTIO_GPU_CMD_RESOURCE_CREATE_2D, flags: 0, fence_id: 0, ctx_id: 0, padding: 0 },
            resource_id,
            format: 1, // B8G8R8A8_UNORM
            width,
            height,
        };
        self.send_cmd_custom(q, &mut cmd as *mut _ as u64, core::mem::size_of::<ResourceCreate2d>() as u32, 0x9999, 24, resp_addr)
    }

    pub unsafe fn attach_backing(&self, q: &mut Virtqueue, resource_id: u32, addr: u64, length: u32, resp_addr: u64) -> u32 {
        let mut cmd = ResourceAttachBackingContiguous {
            hdr: GpuHeader { ctrl_type: VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, flags: 0, fence_id: 0, ctx_id: 0, padding: 0 },
            resource_id,
            nr_entries: 1,
            addr,
            length,
            padding: 0,
        };
        self.send_cmd_custom(q, &mut cmd as *mut _ as u64, core::mem::size_of::<ResourceAttachBackingContiguous>() as u32, 0x9999, 24, resp_addr)
    }

    pub unsafe fn set_scanout(&self, q: &mut Virtqueue, scanout_id: u32, resource_id: u32, width: u32, height: u32, resp_addr: u64) -> u32 {
        let mut cmd = SetScanout {
            hdr: GpuHeader { ctrl_type: VIRTIO_GPU_CMD_SET_SCANOUT, flags: 0, fence_id: 0, ctx_id: 0, padding: 0 },
            x: 0, y: 0, width, height,
            scanout_id,
            resource_id,
        };
        self.send_cmd_custom(q, &mut cmd as *mut _ as u64, core::mem::size_of::<SetScanout>() as u32, 0x9999, 24, resp_addr)
    }

    pub unsafe fn transfer_to_host_2d(&self, q: &mut Virtqueue, resource_id: u32, width: u32, height: u32, resp_addr: u64) -> u32 {
        let mut cmd = TransferToHost2d {
            hdr: GpuHeader { ctrl_type: VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D, flags: 0, fence_id: 0, ctx_id: 0, padding: 0 },
            x: 0, y: 0, width, height,
            offset: 0,
            resource_id,
            padding: 0,
        };
        self.send_cmd_custom(q, &mut cmd as *mut _ as u64, core::mem::size_of::<TransferToHost2d>() as u32, 0x9999, 24, resp_addr)
    }

    pub unsafe fn flush(&self, q: &mut Virtqueue, resource_id: u32, x: u32, y: u32, w: u32, h: u32, resp_addr: u64) -> u32 {
        let mut cmd = ResourceFlush {
            hdr: GpuHeader { ctrl_type: VIRTIO_GPU_CMD_RESOURCE_FLUSH, flags: 0, fence_id: 0, ctx_id: 0, padding: 0 },
            x, y, width: w, height: h,
            resource_id,
            padding: 0,
        };
        self.send_cmd_custom(q, &mut cmd as *mut _ as u64, core::mem::size_of::<ResourceFlush>() as u32, 0x9999, 24, resp_addr)
    }

    pub unsafe fn ping(&self, q: &mut Virtqueue, resp_addr: u64) -> u32 {
        let cmd_info = GpuHeader { ctrl_type: 0x0100, flags: 0, fence_id: 0, ctx_id: 0, padding: 0 }; 
        self.send_cmd_custom(q, &cmd_info as *const _ as u64, core::mem::size_of::<GpuHeader>() as u32, 0x9999, 400, resp_addr)
    }

    unsafe fn send_cmd_custom(&self, q: &mut Virtqueue, addr: u64, len: u32, init_resp: u32, resp_len: u32, resp_addr: u64) -> u32 {
        let ptr = resp_addr as *mut u32;
        *ptr = init_resp;

        let idx = q.avail.idx as usize % 16; 
        let head = (idx * 2) as u16;

        q.desc[head as usize].addr = addr;
        q.desc[head as usize].len = len;
        q.desc[head as usize].flags = VRING_DESC_F_NEXT;
        q.desc[head as usize].next = head + 1;
        
        q.desc[head as usize + 1].addr = resp_addr;
        q.desc[head as usize + 1].len = resp_len;
        q.desc[head as usize + 1].flags = VRING_DESC_F_WRITE;
        q.desc[head as usize + 1].next = 0;
        
        q.avail.ring[q.avail.idx as usize % 32] = head;
        
        vios_driver_virtio::memory_barrier();
        let target = q.avail.idx.wrapping_add(1);
        q.avail.idx = target;
        vios_driver_virtio::memory_barrier();
        
        self.mmio.notify(0);
        
        let mut timeout = 10_000_000;
        while q.used.idx != target && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        core::ptr::read_volatile(ptr)
    }
}

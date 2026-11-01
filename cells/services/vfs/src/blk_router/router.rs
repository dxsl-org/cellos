//! Per-device router state and provider resolution over an abstract [`BlockSys`].
//!
//! Each device entry is the VFS view of one kernel route slot (syscall 432):
//! its key, its name and the provider requests currently go to. A device found
//! through 432 never uses the kernel block path; only [`DeviceRef::Kernel`] does.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use api::block_route::{DeviceKey, ViBlockRouteV1};

use super::discovery::device_name;
use super::{BlockSys, DeviceRef};

/// A live provider as named by a route record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Provider {
    pub tid: usize,
    pub cell: u64,
    pub generation: u64,
}

impl Provider {
    /// The live provider of `record`, if the record still carries `key`.
    fn of(record: &ViBlockRouteV1, key: DeviceKey) -> Option<Self> {
        let (tid, cell, generation) = record.provider().filter(|_| record.device() == key)?;
        Some(Self { tid: tid as usize, cell, generation })
    }
}

pub struct Device {
    pub route: u32,
    pub key: DeviceKey,
    pub name: String,
    /// Provider requests go to; `None` until (re)resolved through 432.
    cached: Option<Provider>,
    /// Provider last reported in the log.
    announced: Option<Provider>,
}

impl Device {
    /// Provider requests currently go to.
    #[cfg(test)]
    pub fn provider(&self) -> Option<Provider> {
        self.cached
    }

    /// The provider a fresh 432 names for this device's slot.
    fn lookup(&self, sys: &mut impl BlockSys) -> Option<Provider> {
        sys.lookup_route(self.route).and_then(|r| Provider::of(&r, self.key))
    }
}

/// Where one request goes: the kernel, `(device index, provider tid)`, or nowhere.
pub(super) enum Target {
    Kernel,
    Provider(usize, usize),
    Refused,
}

/// One request/reply exchange: send refused (nothing reached the provider),
/// sent but no reply from a provider that is still the route's, or replied.
pub(super) enum Exchange {
    Refused,
    Missing,
    Replied,
}

pub struct Router {
    devices: Vec<Device>,
}

impl Router {
    pub const fn new() -> Self {
        Self { devices: Vec::new() }
    }

    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    /// Append the device of route slot `route`, resolved to the provider `record` names.
    pub fn add(&mut self, route: u32, record: &ViBlockRouteV1) -> DeviceRef {
        let key = record.device();
        let provider = Provider::of(record, key);
        self.devices.push(Device { route, key, name: device_name(key), cached: provider, announced: provider });
        DeviceRef::Device((self.devices.len() - 1) as u8)
    }

    pub fn name(&self, dev: DeviceRef) -> &str {
        match dev {
            DeviceRef::Kernel => "kernel",
            DeviceRef::Device(i) => self.devices.get(usize::from(i)).map_or("unknown", |d| d.name.as_str()),
        }
    }

    pub(super) fn target(&mut self, sys: &mut impl BlockSys, dev: DeviceRef) -> Target {
        let DeviceRef::Device(i) = dev else { return Target::Kernel };
        let i = usize::from(i);
        let Some(device) = self.devices.get_mut(i) else {
            sys.log(&format!("[blk-router] device {i}: not in the device table; not issued"));
            return Target::Refused;
        };
        if let Some(provider) = device.cached {
            return Target::Provider(i, provider.tid);
        }
        let Some(provider) = device.lookup(sys) else {
            sys.log(&format!("[blk-router] {}: no live provider; not issued", device.name));
            return Target::Refused;
        };
        device.cached = Some(provider);
        if device.announced != Some(provider) {
            device.announced = Some(provider);
            sys.log(&format!(
                "[vfs] block device {} provider tid={} cell={} gen={}",
                device.name, provider.tid, provider.cell, provider.generation
            ));
        }
        Target::Provider(i, provider.tid)
    }

    /// Drop device `i`'s cached provider if it is still `tid`, so the next
    /// request re-resolves the route instead of addressing a dead provider.
    fn forget(&mut self, i: usize, tid: usize) {
        let device = &mut self.devices[i];
        if device.cached.is_some_and(|p| p.tid == tid) {
            device.cached = None;
        }
    }

    /// A receive masked to the provider completes both for a reply and for the
    /// provider's death (the kernel then writes the exit reason into the buffer).
    /// Death clears the route's provider before any receiver runs, so a message
    /// counts as a reply only if a fresh 432 still names `tid`.
    pub(super) fn exchange(&mut self, sys: &mut impl BlockSys, i: usize, tid: usize, req: &[u8], reply: &mut [u8]) -> Exchange {
        if !sys.send(tid, req) {
            self.forget(i, tid);
            return Exchange::Refused;
        }
        let replied = sys.recv(tid, reply) && self.devices[i].lookup(sys).is_some_and(|p| p.tid == tid);
        if !replied {
            self.forget(i, tid);
            return Exchange::Missing;
        }
        Exchange::Replied
    }
}

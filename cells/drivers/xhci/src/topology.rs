//! USB topology and route string management (xHCI 1.2 §8.9).

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceTopology {
    /// Root hub port number on the controller (1-based).
    pub root_port: u8,
    /// 20-bit xHCI route string.
    pub route_string: u32,
    /// Hub tier depth (0 = direct on root port, 1 = behind 1 hub, 2 = behind 2 hubs).
    pub depth: u8,
    /// Speed ID (1=full, 2=low, 3=high, 4=super).
    pub speed: u32,
    /// Slot ID of the parent Transaction Translator (TT) hub, if applicable.
    pub tt_slot: u8,
    /// Port number on the TT hub, if applicable.
    pub tt_port: u8,
}

impl DeviceTopology {
    pub const fn root(port: u8, speed: u32) -> Self {
        Self {
            root_port: port,
            route_string: 0,
            depth: 0,
            speed,
            tt_slot: 0,
            tt_port: 0,
        }
    }

    /// Compute topology for a device connected to `hub_port` of a parent hub.
    pub fn child_of(&self, hub_slot: u8, hub_port: u8, child_speed: u32) -> Option<Self> {
        if self.depth >= 2 || hub_port == 0 || hub_port > 15 {
            return None;
        }
        let shift = self.depth * 4;
        let route = self.route_string | (((hub_port as u32) & 0xF) << shift);
        let (tt_slot, tt_port) = if child_speed < 3 {
            // Low or full speed: needs TT if hub is high-speed
            if self.speed == 3 {
                (hub_slot, hub_port)
            } else {
                (self.tt_slot, self.tt_port)
            }
        } else {
            (0, 0)
        };
        Some(Self {
            root_port: self.root_port,
            route_string: route,
            depth: self.depth + 1,
            speed: child_speed,
            tt_slot,
            tt_port,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_topology_has_zero_route_string() {
        let topo = DeviceTopology::root(1, 3);
        assert_eq!(topo.root_port, 1);
        assert_eq!(topo.route_string, 0);
        assert_eq!(topo.depth, 0);
    }

    #[test]
    fn tier1_child_route_string_and_tt() {
        let root = DeviceTopology::root(1, 3);
        let child = root.child_of(1, 2, 2).expect("child");
        assert_eq!(child.root_port, 1);
        assert_eq!(child.route_string, 2);
        assert_eq!(child.depth, 1);
        assert_eq!(child.tt_slot, 1);
        assert_eq!(child.tt_port, 2);
    }

    #[test]
    fn tier2_child_route_string() {
        let root = DeviceTopology::root(1, 3);
        let hub1 = root.child_of(1, 2, 3).expect("hub1");
        let dev = hub1.child_of(2, 4, 1).expect("dev");
        assert_eq!(dev.root_port, 1);
        assert_eq!(dev.route_string, 2 | (4 << 4)); // 0x42
        assert_eq!(dev.depth, 2);
    }

    #[test]
    fn depth_greater_than_2_is_refused() {
        let root = DeviceTopology::root(1, 3);
        let h1 = root.child_of(1, 1, 3).unwrap();
        let h2 = h1.child_of(2, 1, 3).unwrap();
        assert_eq!(h2.child_of(3, 1, 1), None);
    }
}

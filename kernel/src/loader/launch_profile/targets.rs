use crate::task::cap::CapSet;

use super::profiles::{
    console_mmio_capset, gpio_mmio_capset, sensor_mmio_capset, spi_demo_mmio_capset,
};

pub(super) fn reviewed_user_target_ceiling(target: &str) -> Option<CapSet> {
    let caps = match target {
        "/bin/audio-demo"
        | "/bin/bench-probe"
        | "/bin/cat"
        | "/bin/cfi-test"
        | "/bin/curl"
        | "/bin/doom"
        | "/bin/echo"
        | "/bin/gpio-test-rv"
        | "/bin/http-smoke"
        | "/bin/input-test"
        | "/bin/ls"
        | "/bin/posix-shim-test"
        | "/bin/ps"
        | "/bin/robot-dashboard"
        | "/bin/tetris"
        | "/bin/tetris-c"
        | "/bin/tetris-lua"
        | "/bin/vfs-test"
        | "/bin/wx-test" => CapSet::EMPTY,
        // These clients and servers use typed IPC to the net service; they do
        // not hold NetworkCap themselves. Keeping their launch ceiling empty
        // also lets exact shell SpawnFromElf edges remain capability-free.
        "/bin/httpd" | "/bin/https-demo" | "/bin/llm-gateway" | "/bin/mqtt" | "/bin/nc"
        | "/bin/wget" => CapSet::EMPTY,
        "/bin/net-broker" => CapSet {
            network: true,
            ..CapSet::EMPTY
        },
        "/bin/periph-demo" | "/bin/periph-test" => console_mmio_capset(),
        "/bin/pwm-demo" => gpio_mmio_capset(),
        "/bin/sensor-demo" => sensor_mmio_capset(),
        "/bin/spi-demo" => spi_demo_mmio_capset(),
        "/bin/bench"
        | "/bin/hotswap-demo-v1"
        | "/bin/hotswap-demo-v2"
        | "/bin/hypha"
        | "/bin/tool-spawn" => CapSet {
            spawn: true,
            ..CapSet::EMPTY
        },
        "/bin/python" | "/bin/lua" | "/bin/tool-fs" | "/bin/tool-sys" => CapSet::EMPTY,
        "/bin/robot-demo" => CapSet {
            network: true,
            mmio_devices: gpio_mmio_capset().mmio_devices,
            ..CapSet::EMPTY
        },
        _ => return None,
    };
    Some(caps)
}

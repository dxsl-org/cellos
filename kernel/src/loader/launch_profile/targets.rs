use crate::task::cap::CapSet;

use super::profiles::{
    console_mmio_capset, gpio_mmio_capset, sensor_mmio_capset, spi_demo_mmio_capset,
};

pub(super) fn reviewed_user_target_ceiling(target: &str) -> Option<CapSet> {
    let caps = match target {
        "/bin/adc-demo"
        | "/bin/ai-test"
        | "/bin/audio-demo"
        | "/bin/bench-probe"
        | "/bin/c2c-render-worker"
        | "/bin/c2c-render-domain-worker"
        | "/bin/can-demo"
        | "/bin/cat"
        | "/bin/cfi-test"
        | "/bin/curl"
        | "/bin/c-pthread"
        | "/bin/cpp-smoke"
        | "/bin/tls-test"
        | "/bin/futex-test"
        | "/bin/pipe-test"
        | "/bin/pipe-peer"
        | "/bin/c-spawn"
        | "/bin/backend-worker"
        | "/bin/doom"
        | "/bin/echo"
        | "/bin/free"
        | "/bin/gpio-test-rv"
        | "/bin/http-smoke"
        | "/bin/input-test"
        | "/bin/window-policy-probe"
        | "/bin/ls"
        | "/bin/posix-shim-test"
        | "/bin/ps"
        | "/bin/robot-dashboard"
        | "/bin/viui-demo"
        | "/bin/tetris"
        | "/bin/tetris-c"
        | "/bin/tetris-lua"
        | "/bin/vfs-test"
        | "/bin/wx-test"
        | "/bin/tier2-exploit"
        | "/bin/tier2-smoke"
        | "/bin/tier2-rpc-provider"
        | "/bin/tier2-rpc-driver"
        | "/bin/std-smoke"
        | "/bin/desktop"
        | "/bin/ocel"
        | "/bin/ocel-js"
        | "/bin/ocel-quickjs" | "/bin/ocel-pdf" => CapSet::EMPTY,
        // These clients and servers use typed IPC to the net service; they do
        // not hold NetworkCap themselves. Keeping their launch ceiling empty
        // also lets exact shell SpawnFromElf edges remain capability-free.
        "/bin/httpd" | "/bin/https-demo" | "/bin/llm-gateway" | "/bin/mqtt" | "/bin/nc"
        | "/bin/wget" => CapSet::EMPTY,
        "/bin/net-broker" => CapSet {
            network: true,
            ..CapSet::EMPTY
        },
        "/bin/dwc2-usb" => CapSet {
            usb_driver: true,
            ..CapSet::EMPTY
        },
        // Tier 3 on demand. Cellos brings up Tier 1 and 2 and lands on a shell;
        // the guest is a workload the operator starts (`hv`), so the shell needs
        // exactly the authority the init edge gives this cell and nothing more.
        // The Elf route still refuses a non-empty child ceiling, so only the
        // VIFS1-resident `/bin/hypervisor` can be launched through this row.
        "/bin/hypervisor" => CapSet {
            hypervisor: true,
            ..CapSet::EMPTY
        },
        "/bin/periph-demo" | "/bin/periph-test" => console_mmio_capset(),
        "/bin/pwm-demo" => gpio_mmio_capset(),
        "/bin/sensor-demo" => sensor_mmio_capset(),
        "/bin/spi-demo" => spi_demo_mmio_capset(),
        "/bin/bench"
        | "/bin/c2c-render"
        | "/bin/capacity-probe"
        | "/bin/hypha"
        | "/bin/tool-spawn"
        // The B0 supervisor witnesses the actor/supervisor library: it must reach
        // the shell edge with its SpawnCap intact, or it cannot spawn or watch
        // children at all (ADR-0021 §2.4).
        | "/bin/backend-supervisor"
        | "/bin/hotswap-demo-v1"
        | "/bin/hotswap-demo-v2" => CapSet {
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

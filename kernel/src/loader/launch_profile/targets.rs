use crate::task::cap::CapSet;

use super::profiles::{
    console_mmio_capset, gpio_mmio_capset, sensor_mmio_capset, spi_demo_mmio_capset,
};

/// The pre-annex X/M/W rights every reviewed non-sandbox row keeps (annex
/// S2-T/S2-W §3, D-TW1). A future restricted row is written without it.
const LEGACY: CapSet = CapSet::LEGACY_OBSERVE_MUTATE;
/// Outbound network client: `Resolve` plus outbound sockets (annex S2-N §5).
pub(super) const NET_CLIENT: CapSet = CapSet {
    net_resolve: true,
    net_connect: true,
    ..LEGACY
};
/// Inbound-only server: listening sockets, no DNS and no outbound connect.
const NET_LISTENER: CapSet = CapSet {
    net_listen: true,
    ..LEGACY
};
/// Client and server in one Cell (`nc -l`, the cluster broker).
const NET_PEER: CapSet = CapSet {
    net_resolve: true,
    net_connect: true,
    net_listen: true,
    ..LEGACY
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
        | "/bin/ocel-js"
        | "/bin/ocel-quickjs" | "/bin/ocel-pdf" | "/bin/chrome" | "/bin/vm-runner" | "/bin/files" | "/bin/terminal" => LEGACY,
        // Typed-IPC clients and servers of the net service. They hold no
        // NetworkCap; their ceiling carries only the coarse network rights the
        // net service enforces from the attested trailer. A non-empty ceiling
        // means the caller-owned SpawnFromElf route refuses these edges, so a
        // shell cannot relabel arbitrary bytes as a granted worker.
        "/bin/curl" | "/bin/wget" | "/bin/mqtt" | "/bin/ocel" | "/bin/llm-gateway"
        | "/bin/https-demo" => NET_CLIENT,
        "/bin/nc" => NET_PEER,
        "/bin/httpd" => NET_LISTENER,
        "/bin/net-broker" => CapSet {
            network: true,
            ..NET_PEER
        },
        "/bin/dwc2-usb" => CapSet {
            usb_driver: true,
            ..LEGACY
        },
        // Tier 3 on demand. Cellos brings up Tier 1 and 2 and lands on a shell;
        // the guest is a workload the operator starts (`hv`), so the shell needs
        // exactly the authority the init edge gives this cell and nothing more.
        // The Elf route still refuses a non-empty child ceiling, so only the
        // VIFS1-resident `/bin/hypervisor` can be launched through this row.
        "/bin/hypervisor" => CapSet {
            hypervisor: true,
            net_l2: true,
            ..LEGACY
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
            ..LEGACY
        },
        "/bin/python" | "/bin/lua" | "/bin/tool-fs" | "/bin/tool-sys" => LEGACY,
        // Private S2-T/S2-W positive control (annex §6, E1): the same probe binary
        // the sandbox copy at `/data/bin/s2tw-probe` runs, on a reviewed edge that
        // holds X/M/W. Fixture feature only; never in an ordinary image.
        #[cfg(feature = "phase04-driver-profile")]
        "/bin/s2tw-probe" => LEGACY,
        "/bin/robot-demo" => CapSet {
            network: true,
            mmio_devices: gpio_mmio_capset().mmio_devices,
            ..NET_CLIENT
        },
        _ => {
            // Option A: Path-based User App Sandbox
            // Any user application in /apps/, /data/bin/, or /home/ is automatically
            // authorized with CapSet::EMPTY (pure IPC client, zero ambient hardware
            // authority, no network rights — annex S2-N decision D-N2 — and no
            // task/memory observation or VFS mutation — annex S2-T/S2-W D-TW1).
            if target.starts_with("/apps/") || target.starts_with("/data/bin/") || target.starts_with("/home/") {
                return Some(CapSet::EMPTY);
            }
            return None;
        }
    };
    Some(caps)
}

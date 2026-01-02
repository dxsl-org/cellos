# Flexible Init System & Boot Modes Architecture

## 1. Philosophy: Decoupled & Adaptable
ViOS adopts a strict separation between the **Kernel Core** and the **User Interface (Shell/GUI)**. Unlike traditional monolithic systems that tightly couple the text console or graphics subsystem with the kernel boot process, ViOS Kernel's responsibility ends at initializing hardware and spawning the first user-space process (`init`).

This design allows ViOS to span across the entire computing spectrum: from localized embedded controllers and cloud microservices (Headless) to full-featured workstations (Desktop).

## 2. The Init Process
The `init` process in ViOS is "Configuration Aware". Upon startup, it reads `/etc/init.conf` or kernel command-line arguments to determine the execution mode.

```rust
// Pseudo-code logic for /bin/init
fn main() {
    let mode = config::get_boot_mode(); // Reads from /etc/init.conf or Kernel Cmdline
    
    match mode {
        BootMode::Desktop => {
            // Standard OS behavior
            service::start("window_server");
            service::start("login_manager");
        },
        BootMode::Kiosk(app_path) => {
            // Hardware Appliance / ATM machine
            service::start("window_server_lite");
            process::spawn(app_path); // e.g., /bin/pos_terminal
            // If app dies, restart it or panic
        },
        BootMode::Headless => {
            // Cloud / Embedded / Server
            // NO GUI, NO SHELL on console by default
            service::start("networking");
            service::start("remote_management_agent"); // SSH or API
            service::start("target_microservice");
        }
    }
}
```

## 3. Supported Boot Modes

### Type 1: Interactive (Desktop Mode)
*   **Target**: Personal Computers, Dev Workstations.
*   **Chain**: Kernel -> Init -> Window Server -> Desktop Environment (Shell/Launcher).
*   **Characteristics**: 
    *   Full GUI stack loaded.
    *   Multi-window management.
    *   High resource consumption relative to others.

### Type 2: Kiosk Mode (Single App)
*   **Target**: ATMs, Public Displays, Industrial Control Panels, Game Consoles.
*   **Chain**: Kernel -> Init -> Window Server (Passthrough) -> Single App.
*   **Characteristics**:
    *   **No Window Management**: The application draws directly to the provided surface which maps to the full screen.
    *   **Security**: Impossible for users to "minimize" or "exit" to a desktop.
    *   **Resiliency**: Auto-restart of the application upon crash.

### Type 3: Headless Mode (Cloud & Embedded)
*   **Target**: Cloud Microservices, API Servers, IoT Sensors, Routers.
*   **Chain**: Kernel -> Init -> Application Daemon.
*   **Characteristics**:
    *   **Zero GUI Overhead**: No Window Server process is started. No framebuffer memory is allocated for userspace.
    *   **Zero Shell Overhead**: Even the text-based Shell is optional. The system can boot directly into a database engine or HTTP server.
    *   **Management**: 
        *   Primary: Remote via RPC/REST API/SSH.
        *   Emergency: Serial Console (TTY) can be enabled if needed.
    *   **Cloud Native**: Ideal for VMs or Containers where "displaying" anything is wasted CPU cycles.

## 4. Cloud Microservices Strategy
For cloud deployments, ViOS utilizes **Type 3 (Headless Mode)**.
*   **Efficiency**: By removing the Shell and GUI stack, the OS footprint is minimized to the bare kernel and the application runtime.
*   **Security**: Removing the interactive Shell reduces the attack surface (no "login prompt" to brute force on the physical/virtual console).
*   **Scalability**: Faster boot times due to fewer initialized services, perfect for auto-scaling serverless functions or containerized microservices.

## 5. Implementation Roadmap
1.  **Phase 1 (Current)**: Hardcoded `init` spawning a simple Shell for testing (Monolithic approach for debugging).
2.  **Phase 2**: Implement `init` with configuration parsing.
3.  **Phase 3**: Port `Window Server` logic to userspace (removing Tifflin-style kernel GUI).
4.  **Phase 4**: Define `/etc/init.conf` structure and Service Manager.

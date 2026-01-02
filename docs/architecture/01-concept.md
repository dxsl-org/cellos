# ViOS Architecture: The "Hybrid" Concept

## 1. Philosophy: The "Greedy" Approach
ViOS satisfies three conflicting needs:
- **Safety**: Microkernel isolation keeps the system alive even if components crash.
- **Speed**: Single Address Space (SAS) for native modules allows "God-tier" performance.
- **Compatibility**: Ability to run massive catalogs of existing C/C++ drivers safely.

## 2. The Four Pillars of ViOS

### Layer 1: The "Nucleus" (Core Microkernel)
*   **Tech**: Minimalist Rust-based Microkernel (seL4-like).
*   **Role**: The ultimate guardian. It manages threads, memory protection capabilities, and handles the "Big Red Button" (Reset).

### Layer 2: The "Cells" (Native Rust Performance)
*   **Tech**: Rust Crates in Single Address Space (SAS).
*   **Role**: Zero-latency control for **Critical Systems** (Motor control, IMU sensors, Real-time safety loops).
*   **Mechanism**: Function calls are direct pointers. Isolation is compile-time (Rust Borrow Checker).

### Layer 2.5: The "Silos" (Legacy Compatibility)
*   **Tech**: Isolated Address Spaces + Virtual Machine Monitors (VMM).
*   **Role**: Hosting **Legacy C/C++ Drivers** (WiFi, GPU, Complex USB stacks) that are too risky or complex to rewrite immediately.
*   **Mechanism**: 
    - **Standard Silo**: Driver runs in its own memory space. Communicates via IPC.
    - **Heavy Silo (VMM)**: Driver runs inside a tiny "Mini-Linux" VM if it depends on Linux kernel APIs.

### Layer 3: The "Playground" (WASM Runtime)
*   **Tech**: WebAssembly (Wasmtime).
*   **Role**: The universal sandbox for **Business Logic** and **AI**.
*   **Mechanism**: Runs compiled code (C++, Python, Go) safely.

---

## 3. Revised Architectural Diagram

```mermaid
graph TD
    subgraph Hardware
        CPU
        RAM
        IO_Motors
        IO_WiFi
    end

    subgraph "Layer 1: Microkernel (The Boss)"
        Kernel[Rust Microkernel]
    end

    subgraph "Layer 2: Hybrid Middleware"
        subgraph "Cells (Native Rust - SAS)"
            Motor_Ctrl[Motor Controller]
            Sensor_Fusion[Sensor Fusion]
        end

        subgraph "Silos (Legacy C/C++)"
            WiFi_Driver[WiFi Driver (In Sandbox)]
            GPU_Driver[GPU Driver (In VMM)]
        end
    end

    subgraph "Layer 3: Application (WASM)"
        AI_Brain[AI Logic (Python->WASM)]
        UI_Face[UI (C++->WASM)]
    end

    %% Interactions
    Kernel -->|Manage| Hardware
    
    Motor_Ctrl <-->|Direct Call (Fast)| Sensor_Fusion
    
    WiFi_Driver <-->|IPC (Safe)| Kernel
    WiFi_Driver <-->|IPC (Bridge)| Sensor_Fusion
    
    AI_Brain -.->|Sandbox Call| Motor_Ctrl
```

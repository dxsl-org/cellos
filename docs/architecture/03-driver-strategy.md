# ViOS Strategy: Driver Silos & Legacy Compatibility

## 1. The Strategy: "Contain, Don't Trust"
We cannot afford to rewrite millions of lines of Linux drivers in Rust immediately. Instead, we use a **"Driver Silo"** architecture to safely utilize existing C/C++ code.

### Core Principle
*   **Native Rule**: If it can kill the robot (Motors, Balance), rewrite in Rust.
*   **Legacy Rule**: If it's complex and peripheral (WiFi, BT, GPU), cage it.

---

## 2. Implementation Models

### Model A: "The Silo" (Isolated Process)
For drivers that can run standalone (freestanding C) or with minimal libc.
*   **Structure**: The driver runs in a dedicated userspace process (Memory Protection Domain).
*   **Communication**: 
    1.  **IPC Bridge**: A Rust `Proxy` in the main OS sends commands via IPC.
    2.  **Shared Memory**: Large data buffers (Framebuffers, Network Packets) are shared via mapped memory regions to minimize copying.
*   **Safety**: If the driver Segfaults, only the Silo dies. The Kernel restarts it seamlessly.

### Model B: "The Heavy Silo" (VMM / Mini-Linux)
For drivers that deeply depend on Linux Kernel APIs (e.g., specific `kmalloc` behaviors, `Netlink`, `sysfs`).
*   **Structure**: We spawn a lightweight Virtual Machine (VMM).
*   **Content**: A stripped-down Linux Kernel (<5MB) + The Driver.
*   **Overhead**: Higher RAM usage, but 100% compatibility.
*   **Use Case**: NVIDIA GPU drivers, Proprietary WiFi firmware wrappers.

### Model C: "The Bridge" (FFI + Unsafe)
For high-performance legacy code that *must* run closer to the metal.
*   **Technique**: Use Rust `bindgen` to create bindings.
*   **Safety Wrapper**: Wrap raw FFI calls in `unsafe` blocks within a Rust Struct that enforces invariants.
*   **Deployment**: This code runs *inside* a restricted Silo, never alongside the Kernel.

---

## 3. Prioritization Matrix (The Decision Framework)

| **Driver / Module** | **Complexity** | **Risk** | **Strategy** | **Reasoning** |
| :--- | :--- | :--- | :--- | :--- |
| **Motor Controller** | Low | **Critical** | **Native Rust** | Must not segfault. Need 0 latency. |
| **IMU / Gyro** | Low | **Critical** | **Native Rust** | Data integrity is paramount. |
| **Camera (UVC)** | Medium | Medium | **Silo (Model A)** | Complex USB stack, but manageable in user space. |
| **WiFi / Bluetooth** | High | Low | **VMM (Model B)** | Dependencies on Linux Network stack are massive. |
| **NVIDIA / GPU** | Extreme | Low | **VMM (Model B)** | Closed source blobs require Linux environment. |
| **AI Computer Vision** | High | Low | **WASM** | Heavy compute, but purely logical. Isolate in WASM. |

---

## 4. Development Workflow for Legacy Drivers

1.  **Scout**: Identify the target C/C++ driver source.
2.  **Isolate**: Compile it as a static library or standalone binary (ELF).
3.  **Bridge**: Write a small Rust "Shim" that translates ViOS IPC messages into calls the C driver understands.
4.  **Integate**:
    *   If simple -> Wrap in `Silo`.
    *   If complex -> Build a `RootFS` for `VMM`.
5.  **Deploy**: Load via the ViOS Capability Manager.

## 5. Benefits of this Approach
*   **Time-to-Market**: Get WiFi and Camera working in days, not months.
*   **Security Update**: Fixing a vulnerability in `OpenSSL` (used by WiFi) just means updating the Silo, not recompiling the Kernel.
*   **Progressive Rewrite**: We can replace the "WiFi VMM" with a "Native Rust WiFi" driver in version 2.0 without changing the API used by the rest of the system.

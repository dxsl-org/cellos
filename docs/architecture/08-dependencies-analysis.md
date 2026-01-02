# 🛠️ Development Tools & Dependency Analysis

## 1. Cargo Tools Integration
The following tools have been identified as essential for ViOS development. A setup script `scripts/install_tools.ps1` has been created to install them.

| Tool | Purpose in ViOS |
|------|----------------|
| `cargo-bloat` | Analyze binary size (critical for Kernel/Bootloader). |
| `cargo-asm` | View generated assembly (verify zero-cost abstractions). |
| `cargo-modules` | Visualize crate structure and visibility. |
| `cargo-audit` | Check for security vulnerabilities in dependencies. |
| `cargo-expand` | Inspect macro expansion (useful for `#[entry]` or macros). |
| `cargo-fuzz` | Fuzz testing for robustness (Syscall entry points). |
| `cargo-binutils` | Provides `rust-objdump`, `rust-nm` (essential for bare-metal debugging). |

## 2. Library Analysis: Tokio, Anyhow, Thiserror

### ❌ Tokio
**Recommendation**: **Do NOT Use.**
*   **Reason**: `Tokio` is an asynchronous runtime designed to run **on top of** an Operating System (Linux, Windows, macOS).
*   **Context**: ViOS **IS** the Operating System. We are building the Scheduler and Event Loop (in `kernel/src/process/scheduler.rs`). Using Tokio would require porting it to run on... nothing (bare metal), which defeats the purpose of writing our own kernel.
*   **Alternative**: We act as the runtime. We can implement `Future` traits for our devices if we want async/await syntax in the kernel.

### ⚠️ Anyhow
**Recommendation**: **Allowed ONLY in User Apps & Tools.**
*   **Rule**: Do NOT use in Kernel, Drivers, or `ostd` (libs).
*   **Reason**: `anyhow::Error` is a dynamic error type that requires heap allocation and obscures error details. It is perfect for top-level application logic (Apps) where you just want to "propagate and print" errors.
*   **Use Case**: High-level User Apps (e.g., `vios-shell`, `vios-editor`) and Host Tools (`scripts`).

### ✅ Thiserror
**Recommendation**: **Recommended for Libraries & Kernel.**
*   **Rule**: Safe to use in Kernel (`no_std`), Drivers, and Libraries (`ostd`).
*   **Reason**: `thiserror` allows creating precise, machine-readable Error Enums (e.g., `SyscallError`) without overhead. It works purely at compile time.
*   **Use Case**: Defining core error types in `ostd`, `kernel`, `hal-core`.

## 3. Decision Matrix

| Library | Kernel (Bare Metal) | Drivers (Wasm/Native) | Userspace (Shell/Apps) | Host Tools |
|:---:|:---:|:---:|:---:|:---:|
| **Tokio** | ⛔ | ⛔ | ⚠️ (Future) | ✅ |
| **Anyhow** | ⛔ | ⛔ | ✅ **Recommended** | ✅ |
| **Thiserror**| ✅ **Recommended** | ✅ | ✅ | ✅ |

## 4. Security & Networking: Ockam Analysis

### ❓ What is Ockam?
Ockam is a suite of Rust libraries for building identity, trust, and encrypted messaging (E2EE) between devices.

### 🛡️ Suitability for ViOS
*   **Architecture Fit**: **Excellent**. ViOS is designed for Robotics and IoT. Security (Mutual Authentication) is critical.
*   **`no_std` Support**: **Yes**. Ockam has `####_core` crates designed for embedded targets.
*   **Integration Strategy**:
    *   **Do NOT** link Ockam directly into the Kernel (keep Kernel minimal).
    *   **DO** implement Ockam as a **System Service** (User-space App).
    *   **Workflow**:
        1.  App (e.g., Camera) sends raw data to Ockam Service via IPC.
        2.  Ockam Service encrypts it (Secure Channel).
        3.  Ockam Service sends encrypted packet to Network Driver.

### ✅ Recommendation: **ADOPT (Future Phase)**
Use Ockam for the **Networking Subsystem** to ensure that all communication between ViOS devices (e.g., Robot A <-> Robot B) is encrypted by default. This makes ViOS a "Secure-by-Design" OS.

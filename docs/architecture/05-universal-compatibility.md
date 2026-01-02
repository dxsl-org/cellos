# ViOS Strategy: Universal Compatibility (The "Chameleon")

## 1. Philosophy: The "Universal OS"
ViOS aims to be the ultimate host, capable of running software from the POSIX, Linux, and Windows worlds.
We adopt a tiered strategy inspired by modern Windows (WSL) and Microkernel architectures:

*   **Tier 1: Native POSIX (High Performance)** - "The Home Ground"
*   **Tier 2: Linux Emulation (WSL1 Style)** - "The Impostor"
*   **Tier 3: Full Virtualization (WSL2 Style)** - "The Sandbox"

---

## 2. Tier 1: Native POSIX (The "Ruột")
Running standard open-source software (Nginx, SQLite, Redis) with 99% native performance.

*   **Mechanism**: A custom `libc` implementation written in Rust (based on `relibc`).
*   **Workflow**: 
    1.  Take standard C source code.
    2.  Compile against `libvios` (our libc).
    3.  Result: A native ViOS binary that speaks directly to the Kernel via optimized IPC, bypassing overheads.
*   **Use Case**: Core system utilities, high-performance databases, custom Jarvis logic.

## 3. Tier 2: Linux Compatibility (WSL1 Style)
Running binary-only Linux applications (ELF files) without recompilation.

*   **Mechanism**: **Syscall Translation**.
    *   The Kernel exposes a "Linux Personality" interface.
    *   When an app calls Linux Syscall `sys_write` (1), ViOS traps it and translates it to `vios::console::write`.
*   **Scope**: Focus on the top 150 most common syscalls (File IO, Network, Process Control).
*   **Pros**: Instant access to the Debian/Alpine repository. Extremely low memory footprint vs VM.
*   **Use Case**: CLI tools (`grep`, `awk`), standard backend services.

## 4. Tier 3: Guest OS Virtualization (WSL2 Style)
Running heavy, complex, or foreign OS applications (Windows Apps, proprietary Linux blobs).

*   **Mechanism**: **Lightweight VMM (Virtual Machine Monitor)**.
    *   Uses Hardware Virtualization (Intel VT-x / AMD-V / ARM EL2).
    *   Runs a full guest Kernel (Windows or Linux) in a secure container.
    *   **VirtIO**: Uses shared-memory VirtIO drivers for near-native Disk/Net performance.
*   **Constraint**: Hardware intensive. Only active on **Server Profile**.
*   **Use Case**: Legacy Windows software, heavy graphical applications.

---

## 5. Implementation Roadmap: "Pay as you go"

### Phase 1: The Foundation (POSIX)
*   Integrate a generic `libc` crate.
*   Port `dash` (shell) and `coreutils` to run natively.
*   *Status: Prerequisite for a usable shell.*

### Phase 2: The Convenience (Introduction of "Linux Personality")
*   Implement `elf-loader` crate to parse Linux executables.
*   Implement `syscall-handler` to map Linux ABI to ViOS internal API.
*   *Target:* Run `busybox` binary from Alpine Linux.

### Phase 3: The Heavy Lifter (VMM)
*   Port `Cloud-Hypervisor` or `Firecracker` logic to ViOS.
*   Implement VirtIO Backends (Block, Net).
*   *Target:* Boot a tiny Linux Kernel inside ViOS.

---

## 6. Architecture Diagram

```mermaid
graph TD
    subgraph "ViOS Kernel Space"
        Kernel[Microkernel]
        Trans_Linux[Syscall Translator]
        VMM_Hypervisor[VMM / Hypervisor]
    end

    subgraph "Tier 1: Native"
        App_Native[Nginx (Recompiled)]
        Lib_Vios[libvios (Rust libc)]
        App_Native --> Lib_Vios --> Kernel
    end

    subgraph "Tier 2: WSL1 Style"
        App_Linux[top (Linux ELF)]
        App_Linux -.->|Syscall| Trans_Linux
        Trans_Linux --> Kernel
    end

    subgraph "Tier 3: WSL2 Style (Server Only)"
        subgraph "Guest VM"
            Guest_Kernel[Linux/Win Kernel]
            App_Win[Legacy App]
        end
        Guest_Kernel -.->|VirtIO| VMM_Hypervisor
        VMM_Hypervisor --> Kernel
    end
```

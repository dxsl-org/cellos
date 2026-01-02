# Filesystem Upgrade Design (Phase 21)

## Overview
ViOS currently supports a custom `MiniFat` (FAT16/32). To support modern workloads, external devices, and high-performance storage, we propose a multi-stage filesystem upgrade plan.

## 1. Virtual File System (VFS) Layer
Create a robust `VFS` abstraction to decouple the kernel from specific filesystem implementations.
*   **Traits**: `Filesystem`, `Inode`, `FileHandle`, `DirectoryEntry`.
*   **Mount Points**: Support mounting different FS types at arbitrary paths (e.g., `/mnt/usb`, `/data`).
*   **Switching**: Allow swapping root FS (e.g., boot with RAMFS, then switch to viFS).

## 2. Supported Filesystems
### A. FAT32 / exFAT (External Devices)
- **Goal**: Interoperability with Windows/Linux/macOS USB drives.
- **Implementation**:
    - Continue improving `MiniFat` for FAT32.
    - Add **exFAT** support (larger files > 4GB).
    - Use `exfat-rs` or custom implementation.

### B. viFS 1 (RedoxFS Port)
- **Source**: Ported from **RedoxFS** (Redox OS).
- **Features**: Microkernel-friendly, Scheme-based, relatively simple linear design.
- **Use Case**: General purpose usage for minimal systems.

### C. viFS 2 (TSF - Theseus Filesystem Port)
- **Source**: Ported from **TSF** (Theseus OS? or Transactional Safe FS?).
- **Features**:
    - **Single Address Space (SAS)** optimizations.
    - **Crash Consistency** (Transactional).
    - **Zero-copy** paths.
- **Use Case**: High-performance, large-scale systems (Databases, Server).

## 3. Direct I/O Support
- **Mechanism**: Bypass Kernel Page Cache for specific applications (Databases).
- **Implementation**:
    - Add `O_DIRECT` flag to `file_open`.
    - Generic VFS handles DMA alignment checks.
    - Driver (`virtio_blk`) DMA directly into User Buffer (Zero-copy).

## Roadmap
1.  **VFS Layer**: Define Traits and Mount Logic.
2.  **viFS 1**: Port RedoxFS logic.
3.  **viFS 2**: Research and Port TSF.
4.  **exFAT**: Add driver.
5.  **Direct I/O**: Optimization.

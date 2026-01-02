# ViOS Strategy: Graphics & GUI Subsystem

## 1. Philosophy: The "Compositor-First" Approach
Unlike traditional OSes that embed GUI logic deep in the kernel (Win32k.sys) or use a monolithic server (X11), ViOS moves the **entire Windowing System to Userspace**.

The Kernel's only job is to provide a linear Framebuffer (via UEFI GOP/VESA) and map it to a trusted process: **The ViOS Compositor**.

---

## 2. Architecture

### 2.1. The Compositor (Native ViOS Shell)
*   **Technology**: Rust Native Application.
*   **Library**: **Slint** (Recommended for lightweight/embedded) or **Iced**.
*   **Role**:
    *   Owns the physical screen.
    *   Draws the "Desktop Environment" (Taskbar, Wallpaper, Shadows).
    *   Composites surfaces from other apps into the final image.

### 2.2. Native Apps (Rust/WASM)
*   **Mechanism**:
    *   App renders UI to an internal Shared Memory Buffer.
    *   App sends IPC `Draw(buffer_handle)` to Compositor.
    *   Compositor blits buffer to screen.
*   **Benefit**: If an app crashes, the GUI stays responsive.

---

## 3. Dealing with Legacy Linux Apps (The Wayland Bridge)

How do we run Firefox or VS Code (Linux versions) on ViOS seamlessly? **We impersonate a Wayland Server.**

### 3.1. The Mechanism
Modern Linux apps use the **Wayland** protocol to talk to display servers.
1.  **Inside the Linux Silo**:
    *   We inject a socket at `/run/user/1000/wayland-0`.
    *   The Linux App (e.g., Gedit) connects to this socket, thinking it's talking to GNOME/KDE.
    *   The App writes its pixels to a Shared Memory file (`memfd`).

2.  **The Bridge (ViOS Side)**:
    *   ViOS implements a lightweight **Wayland Server Protocol** translator.
    *   It receives the surface data from the Linux App.
    *   It wraps this data as a ViOS-native Surface.

3.  **The Result**:
    *   The Linux window appears as just another window managed by the ViOS Compositor.
    *   It has native window decorations (Close/Minimize buttons provided by ViOS).
    *   Clipboards are synchronized seamlessly.

### 3.2. Why Wayland?
*   **Isolation**: Every window is a separate buffer. Perfect for our Cell/Silo architecture.
*   **Performance**: Designed for zero-copy shared memory from day one.
*   **Compat**: It is the standard for almost all modern Linux GUIs.

---

## 4. Implementation Roadmap

### Phase 1: Framebuffer Access
*   Kernel implements `ALLOW_FRAMEBUFFER` syscall.
*   Simple Rust userspace app draws a white pixel on screen.

### Phase 2: The Compositor
*   Port `Slint` to run on ViOS (backend: Software Renderer writing to Framebuffer).
*   Implement basic Window Manager logic (stacking windows).

### Phase 3: The Wayland Bridge
*   Integrate `wayland-server` crate into a ViOS Service.
*   Implement shared memory mapping between Linux VMM and ViOS Compositor.

---

## 5. Visual Diagram

```mermaid
graph TD
    subgraph "Hardware"
        Monitor[Display 1920x1080]
    end

    subgraph "Kernel Space"
        GOP[GOP Driver] -->|Map Memory| Compositor
    end

    subgraph "Userspace: The Compositor"
        Compositor[ViOS Shell (Rust/Slint)]
        Compositor --> Monitor
    end

    subgraph "Native App (WASM)"
        ChatApp[Chat UI]
        ChatApp -->|IPC: Shared Buf| Compositor
    end

    subgraph "Legacy Linux Silo"
        LinuxApp[Firefox]
        WaylandSocket[Wayland Socket]
        LinuxApp -->|Wayland Protocol| WaylandSocket
    end

    WaylandSocket -->|Zero-Copy Bridge| Compositor
```

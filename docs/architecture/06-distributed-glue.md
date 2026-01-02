# ViOS Strategy: The "Glue" (Distributed IPC)

## 1. Philosophy: Network Transparency
In ViOS, the network is an implementation detail, not a barrier.
A function call to a local object and a function call to a remote object look exactly the same in code.
This transforms a fleet of devices into a single **"Super-Computer"**.

## 2. The Mechanism: Distributed Capability Object Model (dCOM)

### 2.1. The "Proxy" Pattern
Every object in ViOS (a Sensor, a File, a GPU context) implements a Rust Trait.
*   **Local Object**: The trait method executes the logic directly.
*   **Remote Object**: The kernel generates a **Proxy** that implements the same trait.

**Code Example:**
```rust
// The Developer writes this:
trait Camera {
    fn take_photo(&self) -> Image;
}

// In the Server's RAM:
let robot_cam: Box<dyn Camera> = device_manager.get("robot_1.front_cam");
let img = robot_cam.take_photo(); // Looks local, but travels over 5G!
```

### 2.2. Under the Hood (The Pipeline)
1.  **Serialization (Zero-Copy)**: The arguments are serialized (using `rkyv` or `bincode`) directly into network buffers.
2.  **Transport**: The Kernel's Network Stack sends the packet via TCP/QUIC to the target device.
3.  **Dispatch**: The Robot's Kernel receives the packet, looks up the real object ID, and executes the method.
4.  **Return**: The result travels back the same way.

## 3. The "Edge-to-Cloud" Workflow

### Scenario: "The Brain and The Body"
*   **The Body (Robot)**:
    *   Runs `vi-kernel-edge` (no_std, minimal).
    *   Exposes `Motor` and `Camera` objects.
    *   Resources: 128MB RAM, Embedded CPU.
*   **The Brain (Server)**:
    *   Runs `vi-kernel-cloud` (Full features).
    *   Running `Llama-4-Omni` AI Model.
    *   Resources: 128GB RAM, RTX 5090.

### Interaction Flow:
1.  **Robot** detects an obstacle (Ultrasonic Sensor).
2.  **Robot** fires an event `ObstacleDetected` (via IPC) to the **Server**.
3.  **Server** receives event, wakes up the AI.
4.  **AI** (on Server) grabs the latest frame from `Robot.Camera`.
5.  **AI** analyzes frame: "It's a cat."
6.  **AI** calls `Robot.Motor.stop()` and `Robot.Speaker.say("Hello Kitty")`.
7.  **Robot** executes.

**Latency Note**: We use **QUIC** (UDP-based) for real-time control to minimize packet-loss delays.

---

## 4. Implementation Details

### 4.1. Interface Definition Language (IDL)
We use pure Rust Traits with a macro `#[vios_interface]`.
```rust
#[vios_interface]
trait MobileBase {
    async fn move_to(&mut self, x: f32, y: f32) -> Result<(), Error>;
}
```
The macro automatically generates the **Server Stub** and **Client Proxy**.

### 4.2. Security (Capabilities)
*   **Authentication**: Each IPC connection is encrypted (TLS 1.3 or WireGuard).
*   **Authorization**: We use "Capabilities". The Server cannot just control *any* robot. It must possess a cryptographic "Handle" (Token) that was explicitly granted by the Robot's owner hierarchy.

## 5. Why this wins?
*   **Unified Logic**: You write the AI logic *once* on the Server. You don't need to write C++ embedded firmware logic to handle "Cat Recognition".
*   **Offloading**: The Robot stays cheap, light, and battery-efficient. The heavy lifting happens where power is abundant.

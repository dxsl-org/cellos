# ViOS Strategy: The "Shapeshifter" Build System

## 1. Philosophy: Single Source of Truth
Instead of maintaining separate OS forks for Robots, Drones, and Servers, ViOS uses a **Monorepo** architecture. 
The OS is not a static binary but a **"Kit of Parts"** that is assembled at compile time based on the target device's role.

## 2. The Mechanics: Rust Workspaces & Features

### 2.1. Workspace Structure
We organize the code into distinct semantic layers:

```text
/vios-monorepo
├── /kernel           # The Microkernel (The immutable Core)
├── /hal              # Hardware Abstraction Layers
│   ├── /hal-core     # Traits (GPIO, UART, SPI definitions)
│   ├── /hal-x86      # Implementation for PC/Server
│   ├── /hal-arm-v8   # Implementation for Raspberry Pi/Jetson
│   └── /hal-esp32    # Implementation for Microcontrollers
├── /drivers          # Device Drivers (depend only on hal-core)
│   ├── /marketing    # Non-essential
│   ├── /motor-ctrl   # Critical
│   └── /wifi-shim    # Silo wrappers
└── /profiles         # The Assembly Instructions
    ├── /robot-mk1    # Config for the Robot
    └── /server-hub   # Config for the Central Brain
```

### 2.2. Conditional Compilation (The Magic)
We use Rust's `#[cfg(feature = "...")]` to conditionally include code.

*   **The Robot Build:**
    Command: `cargo build --target aarch64-unknown-none --features "profile-robot"`
    Result: Kernel + ARM HAL + Motor Driver + Camera Driver. (No GUI, No Database).
    
*   **The Server Build:**
    Command: `cargo build --target x86_64-unknown-linux-gnu --features "profile-server"`
    Result: Kernel + x86 HAL + Virtualization + TCP/IP Stack + AI Inference Engine.

---

## 3. The "Assembly Line" Workflow

### Step 1: Coding (The Component)
You write a generic **Navigation Module**.
- It asks the OS: "Give me the Distance Sensor data". 
- It *doesn't care* if the sensor is real (Robot) or simulated (Server/Simulation Mode).

### Step 2: Configuration (The Profile)
You define a profile in `Cargo.toml`.

**Example: `profiles/robot-scout/Cargo.toml`**
```toml
[dependencies]
kernel = { path = "../../kernel" }
hal = { path = "../../hal/hal-arm-v8" }
driver-motor = { path = "../../drivers/motor-ctrl" }
app-ai-vision = { path = "../../apps/vision" }

[features]
default = ["real-time-scheduler", "panic-abort"]
```

### Step 3: Deployment (The "Inception")
1.  **On Developer Machine:** Run `cargo build -p robot-scout`.
2.  **Output:** A single, highly optimized `.bin` file.
3.  **Flash:** Send over wire/OTA to the robot.

---

## 4. Hardware Abstraction Layer (HAL) Strategy
To make drivers portable, we strictly separate **Interface** from **Implementation**.

*   **`hal-core`**: Defines *Traits* like `FlashLight`.
    ```rust
    trait FlashLight {
        fn on(&mut self);
        fn off(&mut self);
    }
    ```
*   **`hal-esp32`**: Implies it for a physical LED.
    ```rust
    impl FlashLight for Gpio2 { ... }
    ```
*   **`hal-sim`**: Implies it for a 3D Simulator.
    ```rust
    impl FlashLight for VirtualLed { 
        fn on(&self) { send_msg_to_unity("LED_ON"); } 
    }
    ```

## 5. Benefits Summary
*   **Atomic Updates**: Fix a bug in the AI logic? Recompile and both Server and Robot get smarter instantly.
*   **Lean Binaries**: No bloat. The Robot doesn't carry the code for the Server's UI.
*   **Simulation First**: You can compile the *exact same* robot logic to run on your PC (Server Profile) inside a simulator before flashing real hardware.

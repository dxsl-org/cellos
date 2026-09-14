# Spec 24 — Unified Native AI Inference Architecture

> **Status**: Ratified 2026-09-12. Normative architectural specification for on-device AI, Small Language Model (SLM) inference, and accelerator integration across Cellos.
> Governed by [ADR-0003](../decisions/0003-application-tier-taxonomy.md) (Application Tiers), [ADR-0014](../decisions/0014-lab-first-robot-workflows.md) (Robot Workflows), and [ADR-0015](../decisions/0015-dual-mode-hybrid-architecture.md) (Dual-Mode Hybrid Architecture).

> **Implementation status (2026-09-13).** Checkpoints CP-1 (ABI and wire protocol), CP-3 (CPU
> `micro` profile), and the CPU portion of §4.1 Phase 2 are implemented and verified at the `host`
> and `qemu` ceilings: `libs/ai-proto`, `libs/ai-sdk`, `libs/gguf-rs`, `libs/ai-tokenizer`,
> `libs/tensor-math`, `libs/ai-engine`, and `cells/services/ai` registered as `service::AI = 15`.
> CP-2 (Tier 2 GGML) stays blocked on the Tier 2 application admission route, CP-4/CP-5 (NPU/GPU)
> stay behind the G3 accelerator envelope and GPU capability gate, and CP-6/CP-7 remain future work.
> Phase 05 (2026-09-14) added the SentencePiece tokenizer family, control-token matching for chat
> templates, and scaled dot-product attention, and now serves a real checkpoint (llama2.c
> `stories260K`) from `/bin/ai` inside a QEMU Cell. `stories15M` still exceeds a Cell's VA slot
> because weight loading copies tensors; zero-copy loading is the next step.
> Two deviations from §6 are deliberate and recorded, not oversights: `AiClient::prompt` keeps the
> async signature shape but streams through a bounded poll loop (the async reactor is not landed),
> and errors are the richer `AiClientError` with a `From` conversion into `ViError`. Evidence and
> plan: `.agents/260913-2002-g2-level-a-ai-inference/`.

---

## 1. Context & Motivation

Cellos spans heterogeneous hardware topologies:
* **G1 (Robot & Embedded)**: Resource-constrained platforms (Raspberry Pi 3 B+, RV64 SBC, RV32 MCU) requiring strict real-time determinism and small memory footprint (< 100MB).
* **G2 (Server & PC)**: High-throughput x86_64 and multi-core ARM64/RV64 workstations with abundant RAM (4GB–64GB+).
* **G3 (NPU-Native Compute)**: Accelerator-equipped SoCs (Rockchip RK3588, SiFive X390, Hailo) capable of multi-TOPS neural tensor execution.

To support intelligent robotics, local reasoning, and conversational agents without compromising real-time guarantees, Cellos specifies a **Single Unified Native AI Architecture (`libs/ai-sdk`)** spanning all tiers and hardware backends.

### Rejected Alternatives

1. **Rejected: Full VM Guest (Tier 3) as the Primary AI Engine**
   * *Rationale*: While a Linux VM on the Cellos VMM can execute existing runtimes (`llama.cpp`, Candle, Python), it carries heavy memory overhead (requires >= 512MB-1GB RAM for the guest kernel + rootfs), slow cold-boot latency (2–10s), and VirtIO trap overhead. It is strictly a legacy fallback for G2 servers, not the native Cellos AI path.
2. **Rejected: Waiting for G4 Rust `std` PAL as a Prerequisite**
   * *Rationale*: The G4 `rust-std` initiative (`*-unknown-cellos`) targets microservices (`tokio`, `axum`, `serde`) and deliberately omits POSIX `mmap` and `std::os::unix`. Furthermore, Tier 1 SAS strictly forbids `unsafe` (`#![forbid(unsafe_code)]`), conflicting with SIMD vectorization and tensor math intrinsics required by heavy ML libraries (such as Hugging Face Candle). Waiting for G4 `std` is an unviable detour for AI inference.

---

## 2. Unified Native AI Architecture (`libs/ai-sdk`)

Rather than maintaining separate architectures for CPU, GPU, and NPU, Cellos unifies all compute backends under **one dynamic accelerator interface (`InferenceBackend`)**:

```text
+---------------------------------------------------------------------------------------------------+
|                                        CELLOS APPLICATIONS                                        |
|             (Robot Control Loop, Hypha Agent, ViUI Desktop, Shell, System Services)               |
+---------------------------------------------------------------------------------------------------+
                                                  |
                                                  |  Law 1 Frozen API: libs/ai-sdk (AiClient)
                                                  v
+---------------------------------------------------------------------------------------------------+
|                              UNIFIED AI INFERENCE SERVICE CELL                                    |
|                                                                                                   |
|   +-------------------------------------------------------------------------------------------+   |
|   |                        InferenceBackend Trait (Pluggable Devices)                         |   |
|   +-------------------------------------------------------------------------------------------+   |
|             |                                    |                                    |           |
|             v [Backend: NPU]                     v [Backend: GPU]                     v [CPU]     |
|   +--------------------+               +--------------------+               +-----------------+   |
|   |  Vendor NPU Driver |               |  Vulkan / VirtIO   |               | Native CPU      |   |
|   |  - RK3588 (6 TOPS) |               |  - Compute Shader  |               | Engine          |   |
|   |  - X390 / Hailo    |               |  - WGPU / OpenCL   |               |                 |   |
|   |  - Zero-Copy Grant |               |  - Tier 2 Domain   |               | [micro]  [full] |   |
|   |  - 100% CPU Free   |               |  - Shared VRAM     |               | no_std   GGML   |   |
|   +--------------------+               +--------------------+               +-----------------+   |
+---------------------------------------------------------------------------------------------------+
```

### 2.1 Pluggable Device Routing (`DeviceTarget`)

The application requests inference via `AiClient`, specifying a device target or allowing the runtime to probe automatically:

```rust
pub enum DeviceTarget {
    Auto,              // Probes: NPU -> GPU -> CPU SIMD
    Npu(NpuKind),      // RK3588, X390, Hailo
    Gpu(GpuKind),      // Vulkan Compute, VirtIO-GPU, Vendor discrete GPU
    Cpu,               // Multi-core CPU with AVX / NEON / RVV
}
```

* **`Auto` Priority Policy**:
  1. If a compatible NPU driver cell is active: Route to **NPU** (maximum power efficiency, 100% CPU offload).
  2. Else if GPU compute is initialized: Route to **GPU** (high parallel batch throughput).
  3. Else: Route to **CPU** (SIMD-accelerated multi-core execution).

---

## 3. Hardware Acceleration Backends: NPU & GPU

### 3.1 NPU Acceleration (Stage G3)
* **Execution Tier**: **Tier 2 Domain Cell** (per [ADR-0015](../decisions/0015-dual-mode-hybrid-architecture.md)) or **Tier 1 `ffi-posix`** with audited IOMMU/SMMU driver isolation.
* **Runtime**: Vendor-provided C/C++ runtime (e.g., `librknnrt.so` / `librknn_llm.so`) linked against `mlibc` or communicated via kernel ioctl driver.
* **Memory Substrate**: Uses Cellos Storage 2.0 Grant APIs (`GrantAlloc`, `GrantShare`, `GrantRegister` - syscalls 208–216) and `sys_grant_tensor` for zero-copy DMA handoff directly to NPU memory without intermediate CPU copies.
* **Characteristics**: 15–25 tokens/sec on RK3588 for 1B–2B parameter models with zero host CPU burden.

### 3.2 GPU Acceleration (Stage G2/G3)
* **Execution Tier**: **Tier 2 Domain Cell** communicating with the GPU Driver Cell (`cells/drivers/virtio-gpu/` or native discrete GPU driver).
* **Compute Interface**: Vulkan Compute or headless compute shaders executing matrix multiplication kernels (GEMM/GEMV).
* **Memory Substrate**: Shared memory apertures mapped into the Tier 2 Domain page table via `sys_grant_dma` (syscall 233).

---

## 4. Native CPU Engine: Phased Evolution & Hardware Profiles

For devices without NPU/GPU or for fallback CPU execution, the engine provides native, zero-VM execution.

### 4.1 Two-Phase Evolutionary Backend

1. **Phase 1: Fast-to-Production (GGML Core via Tier 2 Domain)**
   * Embeds the minimal C core of GGML (`ggml.c`, `ggml-alloc.c`, `ggml-quants.c`) through `libs/mlibc-shim`.
   * Executed strictly in **Tier 2 (Paged Domain Cell)** with its own hardware page table (`CR3`/`satp`/`TTBR0`).
   * Memory violations in C trigger CPU page faults contained by the hardware MMU. The kernel terminates or restarts the AI Cell without corrupting the shared Tier 1 SAS kernel space.
   * Immediate support for GGUF model quantization formats (`Q4_K_M`, `Q8_0`, `FP16`) on AVX-512 and ARM NEON.
2. **Phase 2: 100% Rust Migration (Rust Native Engine)**
   * GGML components are systematically replaced by native Rust modules:
     1. `libs/gguf-rs`: Native `no_std` GGUF parser.
     2. `libs/ai-tokenizer`: Pure Rust BPE/SentencePiece tokenizer.
     3. `libs/tensor-math`: SIMD-vectorized MatMul and SwiGLU kernels (`core::arch`).
   * **Golden Oracle Validation**: Phase 1 GGML serves as an active bit-exact test oracle to verify the outputs of the Rust replacement kernels during development.
   * **Tier 1 Promotion**: Fully validated, `#![forbid(unsafe_code)]` safe-Rust model implementations become eligible for promotion to **Tier 1 SAS**, enabling lock-free SPSC IPC with sub-microsecond latency.

### 4.2 Hardware-Adaptive Runtime Profiles

| Attribute | Profile: `micro` (Robot / Weak Edge) | Profile: `full` (PC / Server / High-End SBC) |
| :--- | :--- | :--- |
| **Target Hardware** | Raspberry Pi 3 B+, RV64 SBC, 512MB–1GB RAM | x86_64 Server, Workstation, RK3588, >= 4GB RAM |
| **Execution Tier** | Tier 1 SAS (Pure Rust) | Tier 2 Domain Cell (Page Table isolated) |
| **Engine Core** | Minimalist `#![no_std]` + `alloc` Rust engine | Phase 1: GGML Core / Phase 2: Full Rust SIMD |
| **Model Scope** | 15M – 135M parameters (SmolLM-135M, Stories) | 0.5B – 7B parameters (Qwen2.5, Llama-3.2, Mistral) |
| **Memory Ceiling** | 32 MiB – 100 MiB bounded quota | 1 GiB – 8 GiB domain virtual memory |
| **Workload Type** | Intent classification, reflex control, anomaly check | Complex multi-turn reasoning, coding, generation |

### 4.3 Real-Time Protection for Robotics

1. **RT Hart Routing (CPU Affinity)**:
   * Real-time robot control tasks (motor PWM, sensor polling, safety watchdogs) are permanently bound to **RT Harts** (e.g., Core 0) with `Priority::Realtime`.
   * The AI Inference Cell is pinned to **Background Harts** (e.g., Cores 1–3) with lower priority. AI tensor saturation cannot preempt or delay real-time control loops.
2. **Strict Quota & Watchdog Protection**:
   * AI memory consumption is strictly capped by the kernel quota (`cell_quota`).
   * If an inference operation exceeds deadlines or panics, the kernel's **RT Watchdog** reaps and reinitializes the AI Cell without interrupting active physical locomotion.

### 4.4 Hybrid Fallback Model (Edge Mesh Delegation)

For robot nodes operating under the `micro` profile, `libs/ai-sdk` provides transparent hybrid routing:
* **Low Complexity / Fast Reflex**: Handled locally via the `micro` engine (< 10ms).
* **Complex Task / Deep Reasoning**: Forwarded via `net-broker` / Hypha IPC to a G2 Cluster Node or Server (7B–70B model).

---

## 5. Relationship with Stage G4 (`rust-std` PAL)

When Stage G4 delivers the native `rust-std` runtime profile (`*-unknown-cellos` target), the AI SDK leverages standard library facilities to streamline and modernize its internal implementation without altering its architecture.

### 5.1 Facilities Leveraged from G4 `std`

1. **`std::sync` (Mutex, RwLock, Condvar, Arc, OnceLock)**:
   * Replaces custom spinlocks and lock-free raw atomics with futex-backed primitives for KV-cache sharing, multi-hart worker thread synchronization, and model weight reference counting.
2. **`std::thread` & Standard Concurrency**:
   * Standardizes worker pool management for multi-core tensor parallelization (GEMM dispatch) across CPU harts, eliminating ad-hoc thread spawn syscall wrappers.
3. **Ecosystem Crate Unlocks**:
   * **Tokenization**: Enables direct compilation of crates like `tokenizers` (Hugging Face) and `tiktoken-rs` without maintaining custom `no_std` BPE forks.
   * **Agent Tooling**: Enables unmodified use of `serde_json` for LLM structured output and ReAct tool-call parsing.
   * **Sampling Constraints**: Enables `regex` for grammar-based sampling and stop-sequence enforcement.
4. **`std::fs` and `std::io` (BufReader, Read, Seek)**:
   * Replaces manual VFS IPC message sequences with standard streaming file I/O for GGUF/Safetensors headers and chunked weight streaming.
5. **`std::time::Instant`**:
   * Provides nanosecond-resolution timing for token-generation metrics (time-to-first-token, tokens-per-second) and watchdog timeout checks.

### 5.2 Boundaries Retained Outside G4 `std`

G4 `std` does **not** subsume the AI SDK. The following responsibilities remain explicitly owned by the AI SDK architecture:
* **Streaming Grant Substrate**: G4 deliberately omits POSIX `mmap`. The SDK's Zero-Copy Grant streaming mechanism remains the sole vehicle for large weight transfers without memory exhaustion.
* **Tier 2 Domain Isolation**: G4 Tier 1 enforces `#![forbid(unsafe_code)]`. Vectorized SIMD kernels (NEON/AVX) and C-FFI backends remain strictly housed in **Tier 2 Domain Cells** protected by hardware MMU page tables.
* **Hardware Accelerator Control**: NPU and GPU hardware hooks continue to interface directly through Cellos HAL, driver cells, and IOMMU grants.

---

## 6. Law 1 Public Interface Contract

In accordance with **Coding Law 1 (Interface is Sacred)**, the public programming model remains immutable regardless of backend transitions:

```rust
// libs/ai-sdk/src/client.rs

pub struct AiClient {
    service_tid: TaskId,
}

impl AiClient {
    /// Connects to the local or federated AI Inference Service.
    pub fn new() -> ViResult<Self>;

    /// Connects with explicit accelerator device preference.
    pub fn with_device(device: DeviceTarget) -> ViResult<Self>;

    /// Submits an inference prompt, returning an asynchronous token stream.
    pub async fn prompt(&self, req: InferRequest) -> ViResult<TokenStream>;

    /// Generates vector embeddings for semantic search or retrieval.
    pub async fn embed(&self, text: &str) -> ViResult<Vec<f32>>;
}
```

Caller cells communicate with the AI Service over typed IPC:
* Opcodes: `INFER_SUBMIT` (0x0601), `INFER_STREAM_POLL` (0x0602), `INFER_CANCEL` (0x0603).
* Large context windows and prompt payloads utilize zero-copy Grant pages (`sys_grant_share`).

---

## 7. Implementation Checkpoints

| Checkpoint | Scope | Deliverable | Governance Gate |
| :--- | :--- | :--- | :--- |
| **CP-1** | ABI & Wire Protocol | `libs/ai-proto` crate, typed IPC records, capability tokens | Law 1 confirmation (2x user confirmation) |
| **CP-2** | CPU Phase 1 (GGML) | Tier 2 Domain AI Cell with `ggml.c` + `mlibc`, Q4_K_M GGUF loader | ADR-0015 Tier 2 implementation gate |
| **CP-3** | CPU Profile Micro | `#![no_std]` pure-Rust Transformer runner for 15M–135M models | QEMU RV64/ARM64 and RPi3 memory budget validation |
| **CP-4** | NPU Driver Backend | RK3588 NPU driver cell + `sys_grant_tensor` zero-copy pipeline | G3 Accelerator Evidence Envelope |
| **CP-5** | GPU Driver Backend | Vulkan/VirtIO-GPU compute shader backend for parallel matrix math | G2/G3 Graphics & GPU capability gate |
| **CP-6** | CPU Phase 2 (Rust) | Replacement of GGML kernels with bit-exact Safe Rust equivalents | Golden Oracle verification test suite |
| **CP-7** | G4 `std` Modernization | Refactor SDK internals using `std::sync`, `std::thread`, and `tokenizers` | G4 `rust-std` promotion gate |

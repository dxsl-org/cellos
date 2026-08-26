# Phase 04 — `tool-peripheral` (NL robot sensor/actuator control) 🎯 G1 showcase

## Context Links
- [plan.md](./plan.md) · [architecture.md](./architecture.md) · [os-gaps.md](./os-gaps.md)
- [phase-02-tool-protocol.md](./phase-02-tool-protocol.md) · [phase-03-tool-sys-spawn.md](./phase-03-tool-sys-spawn.md)
- Reuse: `cells/demos/robot-demo/` · `cells/drivers/gpio/` · `cells/drivers/i2c-gpio/` · `cells/drivers/pwm-gpio/`
- New crate: `cells/apps/hypha/tools/peripheral/`

## Overview
- **Priority**: P4 — the G1 graduation showcase. Natural-language control of real robot hardware.
- **Status**: 🔜 Ready (plan written 2026-07-12) — not yet implemented.
- **Description**: A `tool-peripheral` tool Cell lets the LLM read the SHT3x
  temperature/humidity sensor over bit-banged I2C and drive a GPIO LED (plus a
  bit-bang PWM channel, cheap bonus), all in natural language:
  - *"what's the temperature?"* → `read_sensor` → SHT3x I2C read (0x44, cmd 0x2C06)
  - *"turn on the LED"* / *"blink the light"* → `set_led` → PL061 GPIO pin write
  - *"set the servo to 50%"* → `set_pwm` → BitBangPwm duty cycle
  This is the first Hypha capability that touches **physical hardware**, and it is
  the strongest LBI least-privilege demonstration: the agent brain (`core`) holds
  **no** GPIO authority — it can only reach hardware by delegating to the one Cell
  the kernel granted `gpio`.
- **Target platform**: **ARM64 QEMU virt** only (PL061 GPIO at `0x0903_0000`,
  where the SHT3x demo runs). On RISC-V/x86 the tool self-degrades to synthetic
  readings (no PL061 MMIO in the allowlist). Real SBC (RPi3 + real SHT3x) is out
  of scope — pending real-board bring-up.

## Key Insights (verified against source)

### The central design problem: capability spawn-chain
P2/P3 had `core` spawn each tool Cell directly and talk by the returned tid
([core/src/main.rs:71-110](../../cells/apps/hypha/core/src/main.rs)). **That path
CANNOT work for `tool-peripheral`.** Spawn-time capability grant is
`granted = requested ∩ spawner_caps` — monotonic downgrade, verified at
[kernel/src/loader.rs:251-261](../../kernel/src/loader.rs#L251):
```rust
Spawner::Root          => requested,                 // init: exempt
Spawner::User(stid)    => requested.intersect(ceil), // ceil = spawner's caps
```
`core`'s manifest is `network+spawn`, **no `gpio`**
([core/src/main.rs:25](../../cells/apps/hypha/core/src/main.rs#L25)). If `core`
spawns a `gpio=true` tool, the intersection **strips `gpio`** → `Pl061Gpio::open()`
returns `PermissionDenied`. Giving `core` a `gpio` cap purely to pass it down
would **defeat the entire showcase** (core would then hold hardware authority the
architecture promises it never has, [architecture.md:106](./architecture.md)).

**Decision — `init` spawns `tool-peripheral` (Root, cap survives) and registers it
under a well-known service id; `core` discovers it via `sys_lookup_service` and
calls it by IPC.** This preserves the least-privilege story end-to-end and mirrors
how every real service already launches
([init/src/main.rs:160-165](../../cells/tools/init/src/main.rs#L160)). It is the
one structural departure from the P2/P3 "core spawns its own tools" pattern, and
it is intrinsic to hardware tools. Logged as **os-gap G19**.

### Ownership / delegation decision for I2C + GPIO
- **`tool-peripheral` links the driver rlibs directly and OWNS the PL061 MMIO —
  app-owns-MMIO, no IPC broker.** It depends on `driver-gpio`
  (`Pl061Gpio::open()` → `sys_request_mmio(0x0903_0000, 0x1000)`,
  [gpio/src/lib.rs:44-47](../../cells/drivers/gpio/src/lib.rs#L44)),
  `driver-i2c-gpio` (`BitBangI2c`, SCL=pin0/SDA=pin1,
  [i2c-gpio/src/lib.rs:35](../../cells/drivers/i2c-gpio/src/lib.rs)), and
  `driver-pwm-gpio` (`BitBangPwm`, channel N = pin N,
  [pwm-gpio/src/lib.rs:49](../../cells/drivers/pwm-gpio/src/lib.rs)). It **is** the
  single owning Cell in [architecture.md:106](./architecture.md); `core` delegates
  to it over `AgentToolRequest`. This matches the established peripheral pattern
  (rlib + app-owns-MMIO, no broker) rather than adding a new IPC layer (KISS/YAGNI).
- **Single `Pl061Gpio`, move-cycled between roles.** There is only one PL061
  controller. `tool-peripheral` holds one `Pl061Gpio` and multiplexes it exactly
  as `robot-demo` does ([robot-demo/src/main.rs:58-73](../../cells/demos/robot-demo/src/main.rs#L58)):
  `BitBangI2c::new(gpio)` for a sensor read → `into_gpio()` to reclaim → direct
  `set_direction`/`write_pin` for the LED → `BitBangPwm::new(gpio)`/`into_gpio()`
  for PWM. Pin plan (no overlap): I2C = pins 0/1, LED = pin 3 (the robot-demo
  actuator pin), PWM = channel/pin 6 (the pwm-demo channel).

### MMIO is held for the Cell's lifetime — there is no release
`MmioRegion` has **no `Drop`** and there is **no `sys_release_mmio`**
([libs/ostd/src/mmio.rs:44-56](../../libs/ostd/src/mmio.rs#L44)). A region is
released **only** when the owning Cell dies (`release_for(cell_id)` on every exit
path, [kernel/src/resource_registry.rs:187](../../kernel/src/resource_registry.rs#L187)).
Consequences:
- `tool-peripheral` opens `Pl061Gpio` **once**, lazily on first use, and holds it
  for its lifetime. Subsequent tool calls reuse the held handle.
- It **must retry on `AlreadyExists`** (bounded yield loop) because init also
  spawns run-once demo cells (`robot-demo`, `periph-demo`, `pwm-demo`,
  `sensor-demo`) that grab PL061 first and release it only when they exit — the
  exact pattern `pwm-demo` already uses
  ([pwm-demo/src/main.rs](../../cells/demos/pwm-demo/src/main.rs), `Err(AlreadyExists) => sys_yield()`).
- The showcase boot config should **not** run the standalone demos concurrently
  with a live agent (they and the tool contend for the one PL061). Logged as
  **os-gap G20** (release syscall is the real fix).

### Graceful degradation (never-die friendly)
On any target without PL061 (`PermissionDenied` — RISC-V/x86, or `gpio` cap
missing), `read_sensor` returns `sht3x::synthetic(tick)` with `"simulated":true`,
and `set_led`/`set_pwm` return a clean `AgentToolResponse::Err` ("no GPIO on this
platform"). The agent still gets a usable answer; nothing panics.

### Shell GPIO holding is NOT a conflict
The shell declares `gpio=true` ([shell/src/main.rs:14]) **only to delegate the cap
to children via manifest intersection** — it never calls `Pl061Gpio::open()` and
holds **no MMIO region** (scout-verified). So the shell does not contend for PL061
with `tool-peripheral`. The real contention is with the run-once demo cells above.

## Tools Exposed (verbs the LLM sees)

| Tool | Args JSON | Result JSON |
|------|-----------|-------------|
| `read_sensor` | `{}` | `{"temp_c":25.3,"humidity_pct":61.0,"simulated":true}` |
| `set_led` | `{"pin":3,"on":true}` | `{"ok":true,"pin":3,"state":"on"}` |
| `set_pwm` | `{"channel":6,"duty_pct":50}` | `{"ok":true,"channel":6,"duty_pct":50}` |
| `read_pin` (optional) | `{"pin":N}` | `{"pin":N,"level":true}` |

`temp_c`/`humidity_pct` are rendered from the fixed-point `Reading`
(`temp_cx10:i32`, `hum_px10:u32`, [robot-demo/src/sht3x.rs:2-9](../../cells/demos/robot-demo/src/sht3x.rs#L2))
as `whole.tenth` decimal strings (no float formatting in `no_std`).

## Architecture

### Cell topology (delta from P3)
```
init (Root)  ──spawn+register(service::HYPHA_PERIPHERAL)──►  tool-peripheral
  │                                                          manifest: gpio=true
  │                                                          owns PL061 MMIO 0x0903_0000
  ▼                                                             ▲
shell (user types "hypha")                                      │ AgentToolRequest IPC
  ▼                                                             │ (postcard)
hypha core  ──sys_lookup_service(HYPHA_PERIPHERAL)──► tid ──────┘
 manifest: network+spawn (NO gpio)
```

### Dispatch path (inside tool-peripheral)
```
AgentToolRequest::Invoke{name,args_json}
  ├─ ensure_gpio()  → lazy Pl061Gpio::open() w/ AlreadyExists retry (cached in state)
  ├─ "read_sensor"  → BitBangI2c::new(gpio); write_read(0x44,[0x2C,0x06],&mut[u8;6]);
  │                    into_gpio(); sht3x::parse → Reading (or synthetic on NACK)
  ├─ "set_led"      → gpio.set_direction(pin,Output); gpio.write_pin(pin,on)
  ├─ "set_pwm"      → BitBangPwm::new(gpio); set_frequency(ch,50); enable; set_duty; into_gpio()
  └─ "read_pin"     → gpio.set_direction(pin,Input); gpio.read_pin(pin)
→ AgentToolResponse::Ok{result_json} | Err{message}
```

### Data flow (end to end)
```
UART "what's the temperature?" → shell → core stdin
 → render_prompt (+SYSTEM_PREAMBLE peripheral verbs) → llm-gateway → mock/LLM
 → LlmReply::ToolCalls[read_sensor] → core.dispatch_tool → route("read_sensor")=peripheral tid
 → AgentToolRequest → tool-peripheral → BitBangI2c read → Ok{"temp_c":..}
 → core appends tool_result → second LLM call → LlmReply::Text → "It's 25.3°C" → UART
```

## Related Code Files
- **Create**: `cells/apps/hypha/tools/peripheral/{Cargo.toml, build.rs, src/main.rs}`
  - deps: `ostd`, `api`, `agent-proto`, `postcard`, `driver-gpio`, `driver-i2c-gpio`,
    `driver-pwm-gpio`, `hal-gpio`, `hal-i2c`, `hal-pwm`, `types`
  - manifest: `declare_manifest!(block_io=false, network=false, spawn=false, gpio=true, uart=false)`
  - syscalls: `declare_syscalls![Send, Recv, Log, RequestMmio, GetTime]`
  - `build.rs`: `cell_build::emit_linker_script();` (same as every tool cell)
- **Modify**: `libs/api/src/abi/syscall.rs` — add `service::HYPHA_PERIPHERAL: u16 = 13`
  (next free id after `GPU_DRIVER=12`; additive, follows `HOTSWAP_DEMO=7` precedent).
  ⚠️ **Law 1**: this is a `libs/api` change. Additive-only (no layout/ABI break),
  but requires the standard `libs/api` additive review before merge.
- **Modify**: `cells/tools/init/src/main.rs` — spawn `/bin/tool-peripheral` and
  `sys_register_service(service::HYPHA_PERIPHERAL, tid)` in the unsupervised
  section (after the demo cells, `Temporary`-style — never restart, holds GPIO).
- **Modify**: `cells/apps/hypha/core/src/main.rs`:
  - `Tools` struct gains `peripheral: usize`, filled by
    `sys_lookup_service(service::HYPHA_PERIPHERAL)` at startup (NOT spawned by core).
  - `route()` maps `read_sensor|set_led|set_pwm|read_pin` → `peripheral`.
  - `SYSTEM_PREAMBLE` gains the peripheral verb docs.
  - add `LookupService` is already in core's `declare_syscalls!` — verify.
- **Modify**: `scripts/format-disk-arm.ps1` — build + copy all 6 hypha cells to the
  ARM disk `/bin` (`hypha-llm-gateway`→`/bin/llm-gateway`, `hypha-core`→`/bin/hypha`,
  `hypha-tool-fs`, `hypha-tool-sys`, `hypha-tool-spawn`, `hypha-tool-peripheral`→
  `/bin/tool-peripheral`). Target `aarch64-unknown-none-softfloat` (matches kernel).
  Logged as **os-gap G21** (Hypha stack was RISC-V-only until now).
- **Modify**: root `Cargo.toml` — add member `cells/apps/hypha/tools/peripheral`.
- **Modify**: `tools/hypha-mock-llm/mock_proxy.py` — P4 tool triggers + text prefixes.
- **Create**: `tests/integration/tests/hypha-p4-boot.rs` — ARM64 spawn-gate test.

## Implementation Steps
1. **`service::HYPHA_PERIPHERAL = 13`** in `libs/api/src/abi/syscall.rs` (Law-1 additive review).
2. **`tool-peripheral` crate**: copy `tool-sys/{Cargo.toml,build.rs}` as the template;
   add the peripheral driver rlib deps. `main.rs` = `CellRuntime::new().no_heartbeat().run(...)`
   handling `Message|RawMessage` → `handle()` → `dispatch()` (mirror
   [tool-sys/src/main.rs:26-49](../../cells/apps/hypha/tools/sys/src/main.rs#L26)).
3. **GPIO state**: hold `Option<Pl061Gpio>` in a struct owned by the run closure
   (not a `static mut`). `ensure_gpio()` lazily opens with a bounded
   `AlreadyExists`-retry loop; caches the handle; returns `&mut Pl061Gpio` or a
   degradation flag. Reuse `args_extract_str` + add `args_extract_u*` / bool parse
   (copy from [tool-spawn/src/main.rs](../../cells/apps/hypha/tools/spawn/src/main.rs)).
4. **`dispatch()`** verbs per the table; SHT3x decode via a local copy of the
   `sht3x::parse`/`synthetic` logic (12 lines) or factor `robot-demo/src/sht3x.rs`
   into a tiny shared rlib — prefer the shared rlib (DRY) if it is cheap; otherwise
   inline (YAGNI). Fixed-point → `"NN.N"` string helper (no float fmt).
5. **`init`**: spawn `/bin/tool-peripheral` + register `HYPHA_PERIPHERAL` after the
   demo cells; treat as `Temporary` (never restart — it is a long-lived hardware
   owner, but a crash should not thrash the PL061).
6. **`core`**: `Tools.peripheral` via lookup; `route()`; `SYSTEM_PREAMBLE` verbs.
7. **`format-disk-arm.ps1`**: add the 6 hypha packages to the build list and the
   `/bin` copy map.
8. **root `Cargo.toml`**: add the new member.
9. **`mock_proxy.py`**: `_tool_call_for` — `temperature|how hot|sensor|humidity` →
   `read_sensor`; `led|light|blink|turn on|turn off` → `set_led`; `servo|pwm|motor`
   → `set_pwm`. `_text_reply` prefixes for the three verbs.
10. **`hypha-p4-boot.rs`**: ARM64 variant of `hypha-p3-boot` (below).
11. **Build**: `RUSTFLAGS="-C relocation-model=pic" cargo build --release -p vicell-kernel
    --target aarch64-unknown-none-softfloat`; `cargo build --release --target
    aarch64-unknown-none-softfloat -p hypha-tool-peripheral` (+ other 5);
    `.\scripts\format-disk-arm.ps1`. Confirm all 6 cells are ET_DYN PIE.
12. **Boot run #5** (manual, mock proxy up) — the live NL round-trip.

## Todo List
- [ ] phase-04 plan doc (this file)
- [ ] `service::HYPHA_PERIPHERAL = 13` (libs/api additive review)
- [ ] `tool-peripheral` crate (Cargo.toml + build.rs + src/main.rs)
- [ ] GPIO lazy-open + AlreadyExists retry + move-cycle multiplexing
- [ ] `read_sensor` / `set_led` / `set_pwm` (+ optional `read_pin`)
- [ ] SHT3x decode reuse (shared rlib or inlined) + fixed-point string helper
- [ ] `init`: spawn + register `/bin/tool-peripheral`
- [ ] `core`: `Tools.peripheral` lookup + route + preamble verbs
- [ ] `format-disk-arm.ps1`: build + copy all 6 hypha cells (aarch64 softfloat)
- [ ] root `Cargo.toml` member
- [ ] `mock_proxy.py` P4 triggers + text prefixes
- [ ] **builds for aarch64-unknown-none-softfloat** — all 6 cells ET_DYN PIE
- [ ] `hypha-p4-boot` integration test (ARM64 spawn-gate) added + passes/SKIPs cleanly
- [ ] boot run #5 verified (host mock proxy + ARM virt) — NL sensor/LED round-trip

## Success Criteria

### CI gate (no LLM — like `hypha-p3-boot`)
`tests/integration/tests/hypha-p4-boot.rs`, running `qemu-system-aarch64 -machine
virt,gic-version=2 -cpu cortex-a57` against `disk_arm_virt.img`:
1. Boot to `ViCell >`.
2. Assert `[tool-peripheral] ready` appeared during init bring-up (init spawned it).
3. `send_line("hypha")` → assert `[hypha/llm-gateway] service ready`, `[tool-fs] ready`,
   `[tool-sys] ready`, `[tool-spawn] ready`, then `you>`.
4. Assert core logged that it resolved the peripheral service (e.g.
   `[hypha] tool-peripheral ready (tid N)` — add this log in core).
5. `send_line("exit")` → `[hypha] bye`.
6. Prereq-gated **SKIP** (not fail) when the aarch64 kernel/disk/qemu are absent
   (`ci_guard`), matching `hypha-p3-boot`'s `prerequisites_ok()`.

### Boot run #5 (manual — the real showcase)
With `python tools/hypha-mock-llm/mock_proxy.py --plain` on the host and Hypha
launched on ARM virt:
```
ViCell > hypha
[tool-peripheral] ready            ← from init, before this
you> what's the temperature?
[hypha] tool: read_sensor
hypha> It's about 25.3°C at 61% humidity (simulated — no sensor on QEMU).
you> turn on the LED
[hypha] tool: set_led
hypha> LED on pin 3 is now on.
you> exit
[hypha] bye
```
Done = the NL→tool→hardware→NL loop completes for `read_sensor` and `set_led`
without panic, and the reply reflects the tool result.

## Risk Assessment

| Risk | Likelihood × Impact | Mitigation |
|------|--------------------|------------|
| **Cap stripped: core spawns gpio tool → PermissionDenied** (🔴 the core design trap) | Was High until designed out | **init spawns + registers**; core only looks up. Verified against loader.rs:251. |
| **PL061 held for life blocks demo cells / vice-versa** (no release syscall) | Med × Med | Lazy open + bounded `AlreadyExists` retry (pwm-demo precedent); showcase config omits standalone demos. os-gap G20. |
| **Hypha not on ARM disk / wrong target** | High × High (won't boot) | Add all 6 cells to `format-disk-arm.ps1`, build `aarch64-unknown-none-softfloat` (matches kernel + disk script). os-gap G21. |
| **PWM bit-bang timing unreliable under QEMU TCG** | High × Low | `set_pwm` returns Ok on register acceptance, not verified waveform; documented as sim-grade. Do not gate CI on PWM output. |
| **Law-1 friction on `service::HYPHA_PERIPHERAL`** | Low × Med | Additive const only; flag for the standard libs/api review. Fallback: register a raw literal id in init (decoupled) if the const add is contested. |
| **Fixed-point → decimal formatting bug (negative temps)** | Low × Low | Reuse robot-demo's `t_sign`/`t_int`/`t_frac` logic ([robot-demo/src/main.rs:98-101](../../cells/demos/robot-demo/src/main.rs#L98)). |
| **`read_sensor` on QEMU always NACKs (no slave)** | Certain × None | Expected — `sht3x::parse` returns `None` on 0xFF MSB → synthetic fallback, `"simulated":true`. Correct behaviour, not a failure. |

### Rollback
Each change is additive and independently revertible:
- Drop the `tool-peripheral` member + init spawn line → stack reverts to P3 exactly.
- The `service::HYPHA_PERIPHERAL` const is unused if init does not register it.
- `format-disk-arm.ps1` hypha additions are copy lines; removing them yields the
  prior peripheral-demo disk. No migration of on-disk data.

## Security Considerations — capability least-privilege analysis

This phase is the **strongest** LBI capability demonstration in Hypha:

| Cell | manifest | Can touch | Cannot touch | Enforced by |
|------|----------|-----------|--------------|-------------|
| `hypha` core | `network, spawn` | LLM, spawn fs/sys/spawn tools, **lookup** peripheral | **GPIO / I2C / PWM / MMIO** | manifest floor + loader intersection ([loader.rs:251]) |
| `tool-peripheral` | **`gpio`** only | PL061 MMIO `0x0903_0000` (allowlist-gated) | net, /data, spawn, all other MMIO | `sys_request_mmio` allowlist + registry exclusivity ([resource_registry.rs:187]) |

- **Least privilege is literal**: `tool-peripheral` requests exactly `gpio=true` and
  the single PL061 region — nothing else. `sys_request_mmio` rejects any base not in
  the per-arch allowlist and any range already owned
  ([mmio.rs:26-37](../../libs/ostd/src/mmio.rs#L26)).
- **The prompt-injection story holds**: even if the LLM is coerced to "fry the GPIO"
  or "read arbitrary MMIO", `core` has **no** MMIO/gpio capability and cannot forge
  one — the kernel strips it at spawn. It can only ask `tool-peripheral`, which
  physically cannot touch anything but PL061.
- **No `unsafe` in the tool**: `#![forbid(unsafe_code)]` holds — all MMIO goes
  through `MmioRegion`'s bounds-checked accessors ([mmio.rs:62-89]); the `unsafe`
  is confined to trusted `ostd`.
- **Owned buffers (Law 2)**: IPC payloads are `Vec<u8>`/`&[u8]` slices consumed
  before buffer reuse; the SHT3x read buffer is a stack `[u8;6]` — no async
  aliasing.
- **DoS surface**: a malicious prompt could spam tool calls; bounded by
  `MAX_TOOL_ROUNDS=5` in core ([core/src/main.rs:35](../../cells/apps/hypha/core/src/main.rs#L35)).
  A wedged bit-bang loop is bounded by the driver's internal iteration caps.

## Next Steps
- **P5** (persistence/memory): conversation + facts to `/data`, context trimming.
- **os-gap G20** (MMIO release syscall) — promote to a module when >1 long-lived
  MMIO owner must coexist with run-once demos on one bus.
- **os-gap G7/G13** (dynamic/name service discovery) — G19 is another consumer;
  a proper app-service registration API would remove the `service::` id add.
- Real-board bring-up (RPi3 + physical SHT3x) — replaces synthetic readings with
  a live bus; `tool-peripheral` is unchanged (same `ViI2c`/`ViGpio` traits).

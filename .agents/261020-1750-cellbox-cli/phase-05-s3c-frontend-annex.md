# Phase 05 S5-3 annex — option C: input-issued frontend capability, shell-owned session

Status: **TEXT APPROVED with changes by the user on 2026-10-23: D-C1 numbers as proposed; D-C2 re-attach included now (§3a); D-C3 `FrontendHost` replaces `TerminalHost`; D-C4 Ssh kind 2 stays Unsupported until Phase 09.** Entry checks E1–E4 resolved (§10). FROZEN; **CP1 APPROVED by the user on 2026-10-23 (“duyệt CP1”) for SHA-256 `41e7f0b1903f418df8d677c008cde8363fe3d1fc427018e1d3cffe889b05dea6`** (digest before this status amendment).

## 1. Goal and principle
Every shell instance, whether on serial, GUI or later SSH or a remote C2C frontend, is the **controller of its own session**. A frontend is only an **attachment** holding a typed, generation-bound capability issued by the resource owner (the input service, ADR-0024 §3–4). That gives one shell code path (`spawn_foreground`, raw leases, TUI) on every surface, and lets the GUI Terminal stop being a session controller.

## 2. Current state (facts)
- SessionV1 selectors **1–22 are allocated** and the service rejects anything outside `1..=22` (`libs/api/src/abi/cell_session.rs:81-87`, `cells/services/input/src/session.rs:283-286`). Status codes are 0–25 (`cell_session.rs:30-57`).
- `Create` (sel 1) accepts `kind <= 2` numerically but rejects every `kind != 0` as `Unsupported` (`input/src/session.rs:343-346`). `AttachFrontend` (sel 2) requires `frontend == attested caller` (`:384-392`). Limits are 64 sessions system-wide and 8 per controller (`:346-347`).
- Child context: grant record of 40 bytes `kind:u16, rights:u16, slot:u16, reserved:u16, owner_service:u64, object{id,generation}, reserved:u64` (`kernel/src/task/child_context.rs:8-14,63-79`).
  - Kind 1 is KernelEndpoint (slots 0–3) and kind 2 is VfsDirectory (slot 4, owner VFS); kinds 3–5 return `Unsupported` (reserved receipts). Slots 5–7 are reserved for VFS/net/telemetry receipts.
  - `CommitChild` validates owner receipts for VFS cwd and the input binding (`kernel/src/task/cell_session.rs:1197-1317`).
- Input authenticates each request by the attested trailer and has **no death watch**; it polls kernel bootstrap state in `maintain` (`input/src/session.rs:784-805`). Kernel owner watches are VFS-only (`kernel/src/task/scheduler.rs:621-647`).
- Today `TerminalHost::open` creates a kind-0 self-attached session and launches cellbox as the foreground child (`libs/terminal/src/host.rs:1-154`). Cellbox context mode (`cells/tools/cellbox/src/context_session.rs`, `executor.rs:52-310`, `main.rs:124-169`) is non-controller.
- The terminal → `/bin/cellbox` edge is Path-only with a `LEGACY` ceiling. Launch profiles do not filter context grant kinds (`launch_profile/mod.rs:31-50`).

## 3. New ABI (append-only; existing selectors, records and statuses unchanged)
**Session frontend kinds** (the `kind` byte in Create): 0 Serial (unchanged), **1 Gui (admitted by this annex)**, 2 Ssh (stays `Unsupported` until Phase 09).

**SessionV1 selectors** (same 24-byte headers, ≤4000-byte records, existing status set):
| Sel | Name | Request body | Result | Authority |
|---:|---|---|---|---|
| 23 | RegisterFrontend | `operation:Ref, kind:u8(=1), flags:u8(=0), rows:u16, cols:u16, reserved:u16` (24) | `frontend:Ref` (16) | any attested Cell. Holder = caller principal. Limits 8 live per holder, 64 system |
| 24 | BindFrontendChild | `frontend:Ref, child:ChildRef` (16+ChildRef) | `receipt:Ref` | frontend holder only. `child` must be a prepared child of an operation the holder issued. One-shot: a frontend binds at most one child |
| 25 | CreateForFrontend | `operation:Ref, bootstrap:Ref, frontend:Ref, flags:u8(=0), reserved:u8, rows:u16, cols:u16, reserved:u16` (56) | same 120-byte result as `Create`, plus the session's `attachment:Ref` for the frontend (136) | caller must be **exactly the committed child bound by sel 24** (attested principal and generation). Caller becomes controller; session kind = the frontend's kind. One-shot per frontend |
| 26 | QueryFrontend | `frontend:Ref` (16) | `state:u32 (0 Pending, 1 Bound, 2 Attached, 3 Closed, 4 Lost), reserved:u32, session:Ref, attachment:Ref, resume:Ref` (56) | frontend holder only. Session, attachment and resume refs are zero unless state 2 |
| 27 | ReleaseFrontend | `frontend:Ref` (16) | empty | holder only. Detaches; the session receives `Lost(4)` and becomes Detached (§3a); the controller keeps the session |
| 28 | ReattachFrontend | `frontend:Ref (new, Pending, same holder), resume:Ref` (32) | `session:Ref, attachment:Ref` (32) | caller must hold both the new frontend and the session's `resume` capability (§3a) |

The frontend holder then uses the **existing** `Resize`(6), `SubmitEvents`(13), paste (14–17), `SubmitRaw`(20), `EndInput`(21) and `Detach`(10) with the returned `attachment:Ref`. The input service accepts them because the attachment's frontend principal is the frontend holder, not the controller. This replaces the `frontend == controller` rule **only for attachments created by sel 25**; kind-0 self-attachment is unchanged.

**Lost reason** (event payload `reason:u32`, today 1..=3): **4 = FrontendLost** (detached or holder died); **5 = FrontendAttached** (re-attach notice, §3a). The shell keeps running and the session becomes **Detached**: its foreground reader gets `Lost(4)` and its input waits rather than ending (no EOF while Detached).

**Child context grant:** kind **6 `InputFrontend`**, slot **8**, `rights=0`, `owner_service = INPUT` service id, `object = frontend Ref`. Kinds 3–5 and slots 5–7 stay reserved. The kernel validates kind 6 with its own receipt. The context grant array grows from 8 to 9 entries (`child_context.rs:8-18,58-81`, today `[Option<Grant>; 8]` with `slot > 7` rejected). `Prepared.receipts` grows from `[Ref; 2]` to `[Ref; 3]`, and `BindOwnerReceipt` admits kind 6 at index 2 with owner service INPUT (`cell_session.rs:75-107,795-809`). Registration validates a new frontend seal (frontend ref + exact child) and has **no** producer-peer side effect, unlike the existing INPUT kind-1 `InputForeground` receipt, which is not reused. At `CommitChild` a context containing kind 6 without a bound, live matching receipt is refused (`InvalidChild`). Kind-1/kind-2 receipt bytes and behavior are unchanged. The child reads it through `from_own_context` (SDK slot enum gains `Frontend = 8`).

### 3a. Re-attach (D-C2)
- On reaching Attached, input mints one **`resume:Ref`** per session, returned to the frontend holder by `QueryFrontend`. It is a typed, generation-bound capability whose **holder is the frontend holder's principal** (the Terminal Cell). It is not a bearer token: a different principal presenting the bytes is refused (`WrongOwner`).
- `ReleaseFrontend`/`Detach` (or a closed window inside a live Terminal Cell) leaves the session **Detached**. The holder may register a new frontend (sel 23) and call `ReattachFrontend` (sel 28) with that frontend plus `resume`. Input then binds the new attachment, emits a `Resize` with the new frontend's size, and the foreground reader receives an attach notice. The old attachment ref is stale (`Stale`).
- **Holder death revokes `resume`.** The Detached session then has no possible frontend and is closed after the controller observes `Lost(4)`. The shell exits on EOF. Re-attach across Cells, from another Terminal process or a remote C2C frontend, would need explicit capability transfer of `resume` under ADR-0024 remote export rules; that is **out of scope** here and goes to the C2C/Phase 09 work.
- Bounds: at most one live attachment per session; Detached sessions still count against the 8/64 session limits; `resume` is reissued (new generation) after every successful re-attach.
- Event payload: Lost reason 4 marks a detach; a re-attach is signalled by a `Resize` plus a new **Lost reason-space value 5 = FrontendAttached** (an informational event, not a failure). It is delivered through the existing control-frame path so the shell can redraw.

## 4. Lifetimes
- **Frontend holder death:** sel 23 records the holder's `operation:Ref`. Input's existing `maintain` pass additionally runs `ResolveBootstrap` on each live frontend's operation (the same kernel RPC it already runs per session). Proven stale/not-found holder → state 4 Lost, `Lost(4)` to the session; unverifiable → retry, as for sessions today.
- **Controller (shell) death:** the session closes as today, and `QueryFrontend` reports 3 Closed. The holder still sees the child's exit through its own operation.
- **Unbound frontends** expire when their holder dies or after `ReleaseFrontend`. A frontend never bound within its holder's operation lifetime is released when that operation ends.
- No kernel owner-watch change: input stays on polling (VFS-only watches are untouched).

## 5. Output path
The frontend does not receive output through the input service. When launching the shell, the holder grants the stdout/stderr **writer** ends of kernel endpoints it reads (kind 1, slots 1/2, as today). The shell, as controller, delegates those same writer ends to each foreground child's slots 1/2, and `CommitChild` duplicates them (`pipe::duplicate_end`, `kernel/src/task/cell_session.rs:1262-1268`). `PrepareForeground`'s `writers[2]` stay what they are today: the stdin/control **producer** writers for INPUT, unrelated to output (E3). Output ordering is the child's own write order on one endpoint. `CELLBOX_STREAMS=merged` (S3) is kept for the GUI so shell diagnostics stay ordered with echo and prompt.

## 6. Consumers and migration (clean cutover)
- `libs/api`: selectors 23–28, records, `LOST_FRONTEND = 4`, `FRONTEND_ATTACHED = 5`, context kind/slot constants.
- `cells/services/input`: frontend table, sels 23–28, Detached state and `resume` caps, the attachment-principal rule, `maintain` for frontend holders, kind-1 sessions.
- `kernel/src/task/{child_context.rs,cell_session.rs}`: kind 6 / slot 8 validation, owner-receipt check generalized to INPUT.
- `libs/ostd/src/child_context.rs`: slot enum.
- `libs/terminal`: `FrontendHost` **replaces** `TerminalHost`. It registers, prepares and launches the shell with grants, binds, waits for Attached, submits input, drains output and releases. `TerminalSession::create_for_frontend` is added.
- `cells/apps/terminal`: uses `FrontendHost`.
- `cells/tools/cellbox`: if a kind-6 grant is present, `CreateForFrontend` and then run the **serial-controller code path**. The context-mode non-controller branches (`context_session.rs` open/classify, `executor.rs` `start_context_session`/`context_mode`/`next_session_event` routing, `main.rs` context-first branch) are deleted. The bare committed-child mode without a frontend stays only if another caller needs it (§8 E2).
- `cells/tests/phase04-terminal-driver`: migrates to `FrontendHost`.
- `phase02` fixtures: unchanged (serial kind 0).

## 7. Verification
- **Host:**
  - Input service tests on the real dispatch for sels 23–28 (including re-attach: wrong holder or stale resume refused; old attachment stale after re-attach; holder death revokes resume and closes the Detached session): wrong holder and wrong child refused; second bind/create refused; kind-2 `Unsupported`; limits; `Lost(4)` on release and holder death; attachment-principal rule accepted only for sel-25 sessions.
  - Kernel tests: kind 6 without receipt is refused; a receipt for another child is refused; the slot/kind matrix.
  - SDK `FrontendHost` record tests.
- **Private images:**
  - Phase04 terminal runner (driver on `FrontendHost`): all existing checks, plus the shell being a controller in the GUI-type session. `top` interactive and `rm -i` work through the frontend, which today works only on serial.
  - GUI runner 6×: existing 197 checks, plus closing the Terminal window and reopening it in the same Terminal Cell re-attaches the same shell, with history and cwd preserved and the screen redrawn; killing the Terminal Cell → shell sees `Lost(4)` then EOF and exits.
  - Then S5-3 proper (`tui-proof` on serial and GUI, negatives) per the [S5-3 proposal](phase-05-s3-proposal.md) §3.
  - Phase02 48, Phase03 60.

## 8. Entry checks before code
- **E1:** the kernel's `CommitChild` owner-receipt path can be generalized by owner service id without changing the kind-2 bytes (read `cell_session.rs:1197-1317`).
- **E2:** whether anything besides the Terminal/driver uses cellbox's non-controller context mode (`ExtRun` children of other hosts). If something does, keep that mode for it; otherwise delete it.
- **E3:** that `PrepareForeground` accepts writers duplicated from a controller's own context stdout grant.
- **E4:** that the input-service `maintain` poll is cheap enough for 64 frontends at the current period.

## 9. Decisions requested (recommendations first)
- **D-C1 Numbers:** *selectors 23–27, context kind 6 / slot 8, Lost reason 4* (recommended; next free values, reserved ranges untouched).
- **D-C2 Re-attach after frontend loss:** *not in this annex* (recommended), or add `AttachToSession(frontend, session)` now for detach/reattach (tmux-like). Useful for C2C remote frontends later, but extra authority surface.
- **D-C3 Old `TerminalHost`:** *replace by `FrontendHost` (clean cutover)* (recommended), or keep both.
- **D-C4 Ssh kind 2:** *stays Unsupported until Phase 09* (recommended).

## 10. Entry check results (2026-10-23, read-only)
- **E1 — conditional pass:** the receipt mechanism extends without touching kind-2 bytes. It needs the 9-entry context array, a third receipt slot, a kind-6 bind rule and a separate seal (folded into §3). The existing INPUT receipt (`InputForeground`, kind 1/slot 0) is foreground-specific and reserves producer peers, so it is not reusable.
- **E2 — pass:** only the GUI Terminal and the private Phase04 driver launch `/bin/cellbox` with a child context. Init autoload launches it without a context (`config/autoload.toml:23-26`). Cellbox's non-controller context-mode branches can be deleted.
- **E3 — annex corrected:** inherited stdout/stderr writers are delegated to foreground children through slots 1/2 (duplicated at commit), not through `PrepareForeground.writers[2]` (stdin/control producers). §5 is amended.
- **E4 — pass (not measured):** `maintain` runs once per input main-loop turn, nominally 100 Hz (10 ms idle tick, `cells/services/input/src/main.rs:140-147,225-234`). 64 sessions plus 64 frontends means at most ~12,800 `ResolveBootstrap` RPCs/s at the limits. This is a polling baseline, not a CPU measurement, and the slice records actual input-service cost on the private image.

## 11. Results (2026-10-23)
- **Implemented:**
  - api: selectors 23–28 with records (`cell_session/frontend.rs`) and context consts.
  - Kernel: 9-grant context; kind 6 / slot 8; third receipt slot; `RegisterBindReceipt` with a 48-byte frontend seal; commit refuses kind 6 without an exact receipt.
  - ostd: `GrantSlot::Frontend`.
  - Input service: frontend table, selectors 23–28, Detached/resume, holder liveness.
  - SDK: `FrontendHost` replaces `TerminalHost`.
  - Terminal app; cellbox (kind-6 → controller path; non-controller context mode deleted).
  - Driver and runners.
- **Deviations (for ratification):**
  1. **§5 output path (approved R):** the shell relays its session rings to the holder's writers instead of delegating them to children, to keep the S5-2 soft reset and merged ordering.
  2. **Reopen UX (approved A):** window close inside a live Terminal Cell releases the frontend, re-creates the window and re-attaches automatically. A user-facing reopen of a running Terminal (desktop single-instance) is **not delivered**; follow-up.
  3. **Epoch 0:** input on selector-25 attachments carries epoch 0 and input substitutes the current epoch. Keys typed during a foreground change go to the reader current at that moment (tty-like); a paste spanning an epoch change fails `WrongEpoch`.
  4. `CreateForFrontend` result is 144 B (`Create`'s real result is 128 B, not 120).
  5. **Additional kernel reads beyond §3, needing ratification:** `InspectPreparedGrant` (op 18) for kind 6 / slot 8, current INPUT issuer only, with a 32-byte projection of preparer ‖ operation owner; and `QueryOperation` (op 9) opened to the INPUT issuer for holder liveness. Both are read-only.
  6. Lost reasons 4/5 were admitted in `libs/api/src/services/session.rs` `validate_event` (it previously rejected >3).
  7. **Kernel:** a foreground child's terminal record is retained for its preparer until the preparer's operation closes. This fixed an exit-status race that reported status 125. These records count toward the 512-record cap until the shell closes each command's operation, which it does per command.
  8. `FrontendHost` releases a closed frontend before closing the launch operation (relaunch `WouldBlock` fix).
  9. The driver's `external-filter-single` now sends EndInput, because a lone external is a real foreground child that reads terminal input.
  10. **Input buffer:** the `InspectPreparedGrant` reply buffer in input was 256 B but the kernel needs ≥672 (the runtime `UnsupportedVersion` root cause). It is now 704 B, and the host fake enforces the minimum.
- **Removed by the parent:** source-text tests added by the implementer (`executor.rs` ×2, `context_session.rs` ×2, terminal `main.rs` ×1). They violated the no-source-text-test rule.
- **Verified (logs `reports/s5-3c-evidence/`):**
  - Host: api 136+2, terminal-sdk 52, app-cellbox 94, terminal 63, service-input 34, kernel 263.
  - **Phase04 terminal runner 380/380 (102 markers)**, including `frontend-controller-session`, `-rm-interactive`, `-top-quit`, `-release-detached`, `-reattach-redraw/cwd/history-preserved`, `-old-attachment-stale`, `-resize-after-reattach`.
  - **GUI 6/6 at 227/227** (5 click + 1 autoload), including the S53C close → reattach scenario.
  - Phase03 60/60; Phase02 48.
  - **Phase02 kernel lane 38/38 `ATOMIC_PUBLICATION` PASS**, including AP-PG, AP-PQ and AP-PI.
- **Not exercised:** frontend holder death on an image (host tests only); cross-Cell re-attach (out of scope); KVM.

- **Ratified by the user on 2026-10-23 (“duyệt”):** deviation 5 (kernel ops 18 `InspectPreparedGrant` kind 6 and 9 `QueryOperation` for the INPUT issuer, read-only) and deviation 7 (foreground child terminal record retained for the preparer until its operation closes). The other deviations were approved during implementation (R, A) or are recorded facts.

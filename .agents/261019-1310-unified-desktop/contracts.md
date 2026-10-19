# Proposed contracts and implementation ownership

Design specification for unified desktop. Product behavior comes from [ADR-0027](../../docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md), [Acceptance Ledger](acceptance.md), and [Baseline Manifest](baseline-manifest.md). Concrete wire layouts, opcodes, field sizes, finite bounds, and consumer censuses are formally specified in [ABI Delta Ledger](abi-delta-ledger.md).

## Owners and authoritative state

| Owner | Authoritative data | Must not own |
|---|---|---|
| Compositor | Surface/grant attach; frame membership/order; selected view; focus/modal tree; work area; configure/snap/fullscreen | App documents, file access grants, app process lifecycle policy |
| Native app | Sessions/documents/terminal state; content; dirty/close consent; per-view min size | Another app's sessions or global shortcuts |
| Desktop | Reviewed app catalog; launch intent routing; theme/background policy; taskbar/Spotlight | A second mutable frame registry or unchecked spawn/kill/power authority |
| Init/existing lifecycle provider | Authorized spawn, exact child exit, system lifecycle | GUI process labels as authority |
| VFS | File/directory identities, rights, backend semantics and operation outcomes | GUI view ownership |
| VMM | Guest/app runtime, device/grant retirement, controlled exports | General Desktop's privilege or direct guest access to host namespace |

## Identities and IPC

- AppId is a reviewed catalog key, never caller-provided path authority. A logical app may have several owner incarnations.
- ViewRef/FrameRef/LaunchRef include compositor/provider incarnation and non-reused local serial. Every command also carries kernel-verifiable caller provenance; TID, title or path alone is not authority. Before exhausting serial space, reject allocation rather than wrap.
- Individual surface cap/Grant remains its owner's object; frame membership does not transfer access. Trusted Desktop can request allowed grouping/control; apps can operate only their views. Ordinary apps cannot enumerate another user's/session's privileged metadata.
- SDK calls use bounded correlated operations and immutable intent. No blocking cross-send cycle on event/render threads, unbounded retries, ambient global target-frame stash or timeout-as-success.
- Notifications are invalidation hints carrying revision; snapshot query resynchronizes after overflow/restart. Snapshot pagination/revision coherence and resource caps must be part of exact wire, not implicit event loss.
- Prefer one frozen-compatible display extension/new approved service message family; do not renumber/reinterpret existing fixed frames or postcard discriminants. All new public fields need endian/size/reserved-byte/UTF-8/error rules.
- Alt+Space dispatch is an authenticated compositor-to-Desktop `OpenSpotlight` activation request bound to the current Desktop incarnation and a non-reused activation ID. It opens/focuses rather than toggles; duplicate delivery is idempotent. Consume the triggering press/repeat/release sequence once and never deliver it as app input. If Desktop is absent/restarting or its bounded activation queue cannot accept the request, report shell-unavailable through trusted compositor feedback, retire the request, and allow a fresh user retry after rebinding; never replay a stale activation into a replacement instance. Test absence/restart/backpressure as well as normal dispatch.

## View/frame state

Proposed view lifecycle: Requested -> Ready -> ClosingRequested -> (Ready on refusal/cancel | Destroying -> Gone). Failure/death can transition to terminal failure with resources retired independently. Dirty/attention/modal/visibility are explicit attributes, not overloaded lifecycle states. A configure sequence is a separate correlated proposal/ACK/commit protocol; stale ACK does not change current placement.

Frame owns ordered ViewRefs and exactly one selected live view when nonempty. Visibility is Normal/Minimized/Fullscreen; placement is Floating/Snapped(target)/Maximized. Fullscreen stores previous placement and floating geometry, not a destructive reparent. Empty frame destruction cancels pending launches into it. Group minimum size is max of member minima, with exact chrome/content-size accounting. Membership change preflights fit; reject or explicitly resize before commit, never publish clipped geometry.
- Group configure is one frame-generation transaction over the exact membership and work-area revision. Stage all required member geometries/Grants while the old frame stays active; commit only after every matching ACK. Refusal, timeout, owner death, membership/work-area change cancels the whole transaction. Retire staged and old resources only after actual ownership/lifetime confirmation; hidden tabs must never expose stale-sized content or pointer transforms.

Dialog/modal links are owner-verified parent ViewRefs. App-owned modal overlays are not independent tabs by default. Modal/attention from an inactive view is signaled without focus theft. System overlays remain distinguishable from untrusted content.

## Launch and new session

1. Caller requests AppId + create/open intent and optional target FrameRef; launcher validates caller/association and target permission.
2. Create a single-use destination LaunchRef bound to intended app owner/authorized spawn, frame and request generation. For an existing app, ask its authenticated session endpoint; for new process, publish intent through approved spawn handoff. Open intent additionally binds immutable resource identity, requested rights, AppId, exact recipient/spawn incarnation and LaunchRef generation as one owned transfer object; no path-only reopen after admission.
3. Only a matching ready view can consume the destination and its resource transfer atomically. App never supplies arbitrary FrameRef adoption rights. Target dies/restarts, user cancels or creation fails -> terminal result, launch token and unconsumed file authority retired/revoked, no stranded tab. No replacement incarnation may inherit it without a fresh authorized intent.
4. Unknown dispatch is reported and reconciled via operation identity; do not spawn a duplicate app or blindly retransmit file authority because a deadline elapsed.
5. Taskbar launch defaults to new frame; `+` alternative targets current live frame. `+` primary is new active-app session; application without that operation opens Applications selection.

## Close, power and failure

- Closing a view requests cooperative consent; process may own other views and must remain alive. Force-stop is separately authorized and warned: resolve the selected view to the exact lifecycle-owner incarnation, enumerate all affected views across frames and bind fresh confirmation to that immutable set. Owner/set change invalidates confirmation; guessed/reused TID cannot become the target.
- Group/system closure gathers prepare-close responses without destroying views. Preparation binds exact generation-qualified ViewRef/owner set, inventory revision and each content revision. Creation, death, edits, reparenting or membership change invalidates preparation. Commit is accepted only against the same frozen/validated set; otherwise reject and re-prepare. Refusal/deadline/unknown cancels normal close; no forced partial data loss. Applications must not perform irreversible close effects in prepare.
- Power is executed by existing authorized system owner only after validating the complete consent-set/inventory revision at the dispatch boundary. Do not claim reboot cancelled if power was already dispatched; record terminal/unknown correctly.
- App death removes that exact incarnation's views/modal/pending work; compositor death invalidates references, apps retain document state and recreate surfaces. Do not restore arbitrary lost sessions or old grants by guessed IDs.

## Appearance

Shared palette roles and geometry are versioned pure data, adapted into existing `ViTheme` (`libs/viui/src/theme.rs`). Desktop owns runtime preference writes; apps only observe. Configuration persistence is through approved existing storage with explicit guarantees, not a new hidden global database. Ocel Reader chooses FollowSystem or explicit shared Light/Dark/Paper. Config/provider restart triggers snapshot refresh; user document state remains app-owned.

### Palette Specifications and Contrast Invariants
- **Light (System Default):** Background `#F5F5F8`, Surface `#FFFFFF`, Border `#D7DAE0`, Text Primary `#14141C` (17.5:1 contrast against surface), Text Secondary `#5F6473` (5.2:1 contrast), Accent `#235AD7`, Tab Active Bg `#FFFFFF`, Tab Inactive Bg `#EBEDF2`, Reader Bg `#FFFFFF`.
- **Dark:** Background `#121218`, Surface `#1C1C26`, Border `#37374B`, Text Primary `#F5F5FA` (13.8:1 contrast against surface), Text Secondary `#9BA0AF` (5.5:1 contrast), Accent `#5082F0`, Tab Active Bg `#262634`, Tab Inactive Bg `#16161E`, Reader Bg `#16161E`.
- **Paper:** Background `#F6F3EB`, Surface `#EFECE3`, Border `#DAD5C8`, Text Primary `#2B2B2A` (12.1:1 contrast against surface), Text Secondary `#605E5A` (5.4:1 contrast against surface), Accent `#9C5B28`, Tab Active Bg `#FAF8F3`, Tab Inactive Bg `#E8E4DA`, Reader Bg `#F6F3EB`.
- **Invariants:** Normal text contrast strictly $\ge 4.5:1$ across all three palettes. Images, PDF pages, video frames, and syntax highlighting retain source color fidelity (never blindly inverted).

### Geometry and Hit Areas
- Session Tab height: 28 DIP (touch target $\ge 28$ DIP, padding horizontal 12 DIP).
- Tab Close Button: 16 DIP icon, 20 DIP active hit area.
- Bottom Taskbar height: 36 DIP fixed at bottom edge ($y = \text{height} - 36$).
- Spotlight Search HUD: 480 DIP width, 320 DIP max height, centered horizontally.
- Focus ring outline: 2 DIP solid stroke using `focus_ring()`.
## Files and preview

Directory paging must encode full filenames unambiguously, bound batch count/bytes and specify concurrent change restart semantics. Handle-relative operations preserve existing owner/right sealing. Metadata absence is unknown/unsupported, never zero/fake permission/time. Capability result distinguishes source preserved, partial destination, completion and unsupported durability. Mutation sequence identity prevents blindly repeating uncertain moves/deletes.
- Copy binds a stable source identity plus revision or exclusive read lease, writes to a distinct destination staging object, syncs/verifies against supported backend semantics, then revalidates source before a move deletes it. Existing destination replacement uses a supported safe namespace commit; cancellation/failure cannot truncate the previous destination. If a backend cannot provide the required source stability or replacement primitive, preserve originals and return explicit unsupported/partial state, never claim successful move. This is runtime coherence, not a new G2-J crash-atomicity guarantee.

Quick Look uses a read-only selected resource and preview generation. Cancellation prevents publishing obsolete content; provider work may still drain. In-flight PDF/decoder memory cannot be freed/reused just because caller cancelled. Isolated worker death and limits are terminal errors, not a fallback to unsafe in-process parsing.

## Terminal and guest controls

Terminal session owns exact stdin/stdout/stderr endpoints and child-control references. Spawn attaches endpoints atomically or fails before child publication; no ambient UART fallback for GUI sessions. Ctrl+C has documented cooperative interrupt handling and truthful unsupported behavior, not implicit global ForceExit. Resize signals current grid; EOF closes the correct producer/consumer rights. ANSI parser/data never becomes shell/system control authority.
- Exactly one terminal-input reader lease exists per session. Shell owns it at prompt; successful foreground spawn transfers it atomically to the foreground command or selected pipeline reader and suspends shell consumption. Exit/failed spawn/death returns it to the live shell. Background commands get EOF/no terminal stdin unless explicitly redirected; no inherited competing read rights.

VMM app session owns VM, guest display and limited file export. Guest app stays fullscreen independently of host frame size. F11 is consumed by host; resize requires guest configure/reallocation/retirement, not framebuffer stretching. Focus loss releases guest pressed state. App dialogs remain in app-only guest shell; app exit never exposes a general Linux desktop.
- Guest adapters implement PrepareClose/CancelClose/CommitClose bound to host ViewRef/VM generation, exact prepared inventory and honest guest dirty/content revision. Prepare cannot terminate VM or retire display/input. Only commit after complete native+guest consent may do so. If a third-party app cannot supply safe preparation, return NeedsUserDecision/Unknown and block normal group/power close until explicit user save/close or separately confirmed force; never invent consent. Validate both native-first and guest-first refusal orderings on the actual supported app.

## Migration and approval gates

- Native app local tab strips and palettes are deleted on migration; no enduring dual native convention. Low-level standalone-surface primitive may remain if genuinely used, not as a product fallback.
- Concrete wire specification is proposed in phase 01 ([ABI Delta Ledger](abi-delta-ledger.md)), with Law-1 CP1 design approval pending explicit candidate-specific ratification. Planning approval is not ABI approval; no public-contract code edits may begin until CP1 is formally approved. Each phase reviews actual implementation delta and runtime evidence at CP2 before commit.
- Resolve inconsistent source/approval projections with integrating owner before frozen edits. No G2-J format/journal, remote/protected promotion, broad path sandbox relaxation or new hardware program is authorized by this plan.

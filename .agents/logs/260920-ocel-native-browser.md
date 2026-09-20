# 2026-09-20 — Ocel Native Document & Web Viewer Implementation

## Why
CellOS G2 (Organization Servers & Office PCs) required a document and web content viewing solution. Full modern browsers like Chrome are too heavy (~256 MB+ RAM, 5-10s VM boot) for reading local Markdown notes, system documentation, or viewing images. A lightweight (<100 ms startup, <1 MB binary), native document viewer named **Ocel** (derived from *Ocellus* — the simple eye) was designed under ADR-0017 to complement Tier 3 Chrome.

## What landed

1. **Architecture Decision Record (ADR-0017)**:
   - Formulated dual browser strategy: **Ocel** for native lightweight document viewing on Tier 2 vs. **Chrome/Firefox** on Tier 3 Linux VM for heavy web apps (Gmail, Docs, YouTube).
   - Documented comparison of QuickJS vs Boa (29x performance gap on bench-v8 favoring QuickJS, refcount GC avoiding UI frame drops, minimal libc requirements).
   - Upgraded ViUI rendering strategy by referencing tiny-skia algorithms rather than porting the heavy crate.

2. **Ocel Application (`cells/apps/ocel/`)**:
   - **UI Shell (`main.rs`)**: 
     - 44px top toolbar with `[Ocel]` logo, editable address bar, and `[Go]` button.
     - Viewport with frustum culling and real-time scrolling.
     - Visual scrollbar on the right edge with proportional thumb and track jumping.
     - 24px bottom status bar displaying document URL, format, node count, and scroll percentage.
     - Navigation History: `[<]` Back and `[>]` Forward toolbar buttons with dynamic enable/disable styling and history stack management.
     - In-Document Search (`Ctrl+F` / `F3`): floating search bar with real-time match count (`current/total`), case-insensitive scanning across all layout boxes, auto-scrolling to matches, and `Enter` cycling.
     - Multi-Tab Bar (`main.rs`): 28px tab bar supporting concurrent tabs, dynamic active tab styling with cyan indicator, title truncation, `[+]` button, `[x]` close button, and keyboard shortcuts (`Ctrl+T`, `Ctrl+W`, `Ctrl+Tab`).
   - **Document Model & Layout Engine (`doc.rs`)**:
     - `DocNode`: Headings (H1–H4), Paragraphs, CodeBlocks, ListItems, Blockquotes, Rules, RawLines, Images, Tables, and Buttons (`DocNode::Button`).
     - Table Layout & Rendering (`doc.rs`): proportional column width distribution, alternating header backgrounds, and cell borders.
     - Interactive Buttons: native rounded button boxes styled with accent backgrounds and border highlights.
     - `StyledSpan`: Inline bold, italic, code pills, hyperlinks, custom colors.
     - Responsive word wrapping to viewport width.
     - Hyperlink hit-testing (`hit_test_link`) and DOM Node hit-testing (`hit_test_node`) mapping mouse coordinates to links or interactive `NodeId` elements.
     - Reactive DOM Event Loop: click events dispatched to Tier 2 `ocel-js`, receiving `DomMutation` batches, applying mutations, and re-rendering in real time.
   - **Parsers (`parser/`)**:
     - `markdown.rs`: CommonMark-like parser supporting headings, lists, code fences, blockquotes, rules, tables (`| col | col |`), and inlines.
     - `html.rs`: Full HTML Tokenizer & DOM Tree Builder constructing a `DocumentArena` with `NodeId`, nested inline styling (`<a>` hyperlinks, `<b>`, `<i>`, `<code>`), tables (`<table>`, `<tr>`, `<th>`, `<td>`), lists, void tag handling, entities, and inline `<script>` extraction.
     - `mod.rs`: Format auto-detection via extension and content sniffing.
   - **Image Decoder (`image/bmp.rs`)**:
     - Pure Rust, zero-dependency 24-bit and 32-bit uncompressed BMP decoder.
     - Converts to BGRA8888 pixel buffers rendered with alpha blending.
   - **Scripting Engine (`js/`) & Gosub-Inspired Tier 2 Bridge (`js/bridge.rs`)**:
     - Gosub-style `JsEngine` and `JsContext` traits separating JS execution from DOM/rendering.
     - `Tier2JsBridge`: IPC bridge delegating script execution and DOM event dispatching to Tier 2 `ocel-js` service over kernel IPC (Spec 17, masked recv).
     - Transparent fallback to in-process `SimpleJsRuntime` if the service is absent or crashes.
   - **Loader (`loader.rs`)**:
     - Kernel capability-backed file loading (`OpenCap`, `ReadCap`, `StatCap`).
     - Built-in virtual documents: `file:///welcome.md` and `file:///help.md`.
     - Error pages (404, read failure) formatted as native Markdown.
   - **Drawing Primitives (`draw.rs`)**:
     - 2D bitmap drawing, solid/stroke rects, FONT8X8 typography, and alpha-blended image blitting.

   - **Networking Client (`net/http.rs`)**:
   - **Typography & Vietnamese Unicode Engine (`font/mod.rs`)**:
     - Real-time diacritic decomposition & composition engine covering all 134 Vietnamese precomposed vowels (`á, à, ả, ã, ạ, ă, ắ, â, ấ, ê, ế, ô, ố, ơ, ớ, ư, ứ, đ, Đ...`).
     - UTF-8 multi-byte `char` iteration in `draw.rs` replacing naive byte slicing.
     - Zero-cost memory overhead (< 1 KB added to binary) with no external TTF bloat.
   - **HTTPS / TLS 1.3 Client (`net/http.rs`)**:
     - Encrypted HTTPS fetching via `ostd::tls` (`tls_connect`, `tls_write`, `tls_read`, `tls_close`) over port 443 with SNI hostname exchange.
     - Unified `parse_url_scheme` supporting both `http://` and `https://` URLs.
     - HTTP/1.0 and HTTP/1.1 client over CellOS Net IPC (`TcpConnect`, `TcpSend`, `TcpRecv`, `TcpClose`).
     - Host resolution for gateway (`10.0.2.2`), dns (`10.0.2.3`), localhost, and IPv4 literals.
     - Header/body parsing and automatic error document generation.

3. **DOM Arena & Engine Abstraction (`libs/dom-arena`)**:
   - **NodeId & DocumentArena**: Gosub-inspired pointer-free DOM tree avoiding circular `Rc`/`RefCell` references and memory leaks.
   - **DOM Mutation Batching (`DomMutation`)**: fine-grained mutation operations (`SetText`, `SetAttribute`, `RemoveAttribute`, `AppendChild`, `RemoveChild`, `SetDocumentTitle`) sent across IPC boundaries.
   - **DOM Event Model (`DomEvent`)**: maps UI interactions (clicks, keypresses) from Tier 1 to Tier 2 listeners.
   - **Wire Protocol**: Postcard-serialized `OcelJsRequest` and `OcelJsResponse` fitting within standard 4 KiB IPC buffer.

4. **Tier 2 JavaScript Engine Service (`cells/services/ocel-js/`)**:
   - Runs in a private hardware MMU-isolated domain (`service::OCEL_JS = 16`).
   - Any runtime faults or panics in JS execution trigger hardware page-fault traps and clean cell termination, leaving Tier 1 Ocel and the SAS entirely intact.
   - Maintains DOM proxy objects, executes scripts, and batches mutations back to Ocel.
5. **Tier 3 Linux VM Input Bridge (`cells/services/hypervisor/`)**:
   - **VirtIO-Input Device (`virtio_input.rs`)**:
     - Implemented `InputDev` (VirtIO DeviceID=18, MMIO slot 4, SPI 20) conforming to VirtIO 1.1 §5.8.
     - Config space queries (`VIRTIO_INPUT_CFG_ID_NAME`, `VIRTIO_INPUT_CFG_EV_BITS`) exposing keyboard, mouse buttons (`BTN_LEFT`, `BTN_RIGHT`, `BTN_MIDDLE`), and relative axes (`REL_X`, `REL_Y`, `REL_WHEEL`).
     - Connected to `eventq` (queue 0) to flush events to the guest kernel with `EV_SYN` reporting and IRQ 20 injection.
   - **Device Tree Node (`dtb.rs`)**:
     - Added `/virtio_mmio@a000800` (slot 4, SPI 20) with `compatible = "virtio,mmio"`.
   - **Event Forwarding (`run_loop.rs`)**:
     - Hooked into `Wfi` and `Preempted` loops to poll CellOS input service and translate `InputEvent::Key`, `MouseMove`, `MouseButton`, and `MouseScroll` directly to the Linux guest.
     - Enables X11, Wayland, and Chromium running inside the Linux guest to receive native hardware-like input.
6. **Desktop & System Integration**:
   - Configured `/bin/ocel-js` in `kernel/src/loader/boot_ceiling.rs` and `kernel/src/loader/launch_profile/targets.rs`.
   - Added `ocel-js` packaging in `gen_disk.ps1` and signature entry in `scripts/sign-policy.py`.
   - Registered `service::OCEL_JS = 16` in `libs/api/src/abi/syscall.rs`.
   - Added `Ocel Viewer` (`(O)`) to Desktop Launcher catalog (`cells/apps/desktop/src/apps.rs`).
   - Configured `/bin/ocel` in `kernel/src/loader/boot_ceiling.rs` and `kernel/src/loader/launch_profile/targets.rs`.
   - Updated signing policy in `scripts/sign-policy.py`.
   - Added packaging entry in `gen_disk.ps1`.
   - Updated `docs/app-development-guide.md` decision tree.

7. **Verification & Metrics**:
   - Cross-compiled clean across all 3 architectures: `x86_64-unknown-none`, `riscv64gc-unknown-none-elf`, `aarch64-unknown-none-softfloat`.
     - `ocel`: **242 KB** (x86_64), **300 KB** (riscv64), **306 KB** (aarch64).
     - `ocel-js`: **53 KB** (x86_64), **63 KB** (riscv64), **62 KB** (aarch64).
   - Zero clippy warnings with `-D warnings`.
   - Clean `cargo fmt`.
   - Pass 100% repository policy verification (`python3 scripts/cellos-sign --check`, 92 crates, 610 files).

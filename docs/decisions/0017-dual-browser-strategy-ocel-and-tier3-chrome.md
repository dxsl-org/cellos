# ADR-0017 — Dual Browser Strategy: Ocel + Tier 3 Chrome

> **Status**: Accepted 2026-09-20.
> **Supersedes**: None. First browser/viewer decision.

## 1. Context

Cellos G2 (Organization Servers & Office PCs) requires web/document access.
A single "native browser" cannot replicate the web platform — Chrome/Firefox
represent ~35M LOC and ~1,100 Web API interfaces accumulated over 20 years.
Attempting to build a compatible browser from scratch is not feasible.

Conversely, running Chrome inside a Tier 3 Linux VM costs 256 MB+ RAM, 5-10s
boot, and an input/display forwarding bridge. For viewing a local Markdown file,
opening a PDF, or rendering an internal dashboard, that overhead is unacceptable.

## 2. Decision

Two separate apps, each purpose-built for its use case:

### 2.1 Ocel — Universal Document Viewer (Tier 2 Native Domain Cell)

A lightweight, instant-on document and content viewer that uses HTML/CSS as its
internal layout format. **Not a web browser.** Does not aim for web platform
compatibility.

| Property | Value |
|---|---|
| Cell name | `ocel` |
| Execution tier | **Tier 2** (Native Domain Cell, MMU-contained) |
| Runtime profile | Cellos `std` + `ffi-posix` (C libraries via POSIX shim/mlibc) |
| Signing | Unsigned OK (Tier 2 admission) |
| Boot target | <100 ms (SAS cell launch; instant-on snapshot eligible) |
| RAM budget | 2-16 MB typical |
| Path | `cells/apps/ocel/` |

**Supported formats:**

| Format | Parser/Decoder | Approach |
|---|---|---|
| Plain text (`.txt`, `.log`, `.rs`, `.c`, `.json`, `.toml`, `.xml`) | Trivial wrap | Monospace render in `<pre>`, optional syntax highlighting |
| Markdown (`.md`) | `pulldown-cmark` (Rust, MIT) | Parse events → DOM nodes → HTML/CSS pipeline |
| HTML/CSS | Custom subset parser | ~40 HTML tags, ~60 CSS properties, taffy layout |
| HTML + basic JS | QuickJS (C, MIT, ES2025) | ~30 DOM bindings, event handlers, `fetch()` stub |
| PDF | MuPDF (C, AGPL) via ffi-posix | Page rasterize → pixel blit to ViCanvas |
| Images (PNG, BMP, JPEG) | `png` crate (Rust) / custom decoders | Decode → BGRA → `draw_image` |
| SVG (subset) | Custom subset parser | Path → ViCanvas path rendering |
| EPUB | Unzip + HTML pipeline | EPUB = ZIP(HTML + CSS + images) |

**Architecture:**

```
Format Decoders ──→ Intermediate Document (styled block tree)
  txt: wrap <pre>        ──┐
  md:  pulldown-cmark     ──┤──→ DOM tree ──→ CSS cascade ──→ taffy layout ──→ ViCanvas paint
  html: subset parser     ──┘                                                      │
  pdf:  MuPDF rasterize   ─────────────────────────────→ pixel blit ───────────────┘
  img:  decode            ─────────────────────────────→ pixel blit ───────────────┘
                                                                                   ▼
                                                                        ViSurface → Compositor
```

**Key technology choices:**

| Component | Choice | Rationale |
|---|---|---|
| JS engine | QuickJS (Bellard, 2026-06, ES2025) | 210 KB binary, ~29x faster than Boa, minimal libc, refcount GC = no UI jank |
| Layout engine | taffy | `no_std`-compatible CSS Flexbox/Grid/Block, proven in Dioxus/Blitz |
| DOM | Custom arena-alloc tree | kuchiki deprecated; SAS/Tier 2 model needs custom tree; <1000 LOC |
| CSS parser | Custom subset | Full CSS parser (cssparser/stylo) is overkill for 60 properties |
| Rendering | ViCanvas (upgraded) | Existing BGRA8888 pipeline; upgrade with AA/path/gradient referencing tiny-skia algorithms |
| HTML parser | Custom subset (v0.1), html5ever port (v0.2+) | Custom for 40 tags = 2 weeks; html5ever port = 3 weeks additional |
| Markdown | pulldown-cmark | Streaming, CommonMark-compliant, MIT, events map directly to DOM nodes |
| PDF | MuPDF via ffi-posix | Best-in-class C PDF renderer, small footprint, AGPL compatible with open-source Cellos |

**Why Tier 2, not Tier 1:**

Ocel parses untrusted content (HTML from internet, PDFs from email, arbitrary
files). Parser bugs = potential code execution. On Tier 1 SAS, such a bug would
compromise all cells. On Tier 2, the MMU private page table contains the blast
radius. Additionally, Tier 2 permits C/C++ FFI libraries (QuickJS, MuPDF) without
requiring `#![forbid(unsafe_code)]` or Ed25519 signing.

**Why Cellos `std`, not `no_std`:**

- Cellos `std` PAL is shipped (alloc, time, args, HashMap, serde, println!)
- `std::fs` and `std::net` fail-closed — Ocel still uses `ostd` service clients
  for actual I/O, so no security surface expansion
- Ergonomics: Ocel is the largest Cellos app; developer experience matters
- `no_std` adds no security on Tier 2 (MMU is the boundary, not LBI)
- Pilot app for `rust-std` runtime profile on Tier 2

**Why QuickJS over Boa:**

| Factor | QuickJS | Boa |
|---|---|---|
| bench-v8 score | 773 | 26.9 (**29x slower**) |
| ES conformance | ~97% | ~95% |
| Binary size | 210-370 KB | ~2 MB + ICU data |
| GC model | Refcount (no UI pause) | Mark-sweep (stop-the-world pauses) |
| Porting effort | 2 weeks | 6-10 weeks |
| 4 MiB heap viability | Comfortable | OOM risk |
| Cellos threading req | None | Needs rayon/dashmap (unavailable) |

Boa's Rust memory safety is not a differentiator because Ocel runs on Tier 2
where the MMU contains C code bugs identically to Rust code bugs.

**ViUI rendering upgrade plan (reference tiny-skia, not port):**

ViCanvas gains capabilities incrementally by implementing algorithms from
tiny-skia's source, not importing the crate. This preserves the existing
BGRA8888 pipeline, avoids format conversion, and adds zero dependencies.

| Phase | Addition | LOC |
|---|---|---|
| R1 | Anti-aliased lines (Wu), `fill_rounded_rect` with AA corners | ~500 |
| R2 | `PathBuilder` + `Path` + scanline rasterizer + `fill` | ~800 |
| R3 | Alpha-blended `draw_image`, gradient fills | ~300 |
| R4 | Scalable font rendering (fontdue integration) | ~400 |
| R5 | Stroke paths, dash patterns | ~400 |

All additions use default trait impls → backward compatible with existing cells.

### 2.2 Tier 3 Chrome/Firefox — Full Web Browser (Linux VM Guest)

A full Chromium or Firefox instance running inside a Tier 3 Linux VM guest,
providing 100% web platform compatibility.

| Property | Value |
|---|---|
| Execution tier | **Tier 3** (VM Guest, Stage-2 hypervisor fence) |
| Guest OS | Alpine Linux (minimal) |
| Browser | Chromium (`apk add chromium`) or Firefox |
| Rendering | Software (`--disable-gpu`), scanout via virtio-gpu → compositor |
| Boot time | 5-10s (Linux init + browser launch) |
| RAM budget | 256 MB+ (VM + Linux + browser) |

**Existing infrastructure:**

- Hypervisor Cell: shipped on ARM64 + x86_64, full VmExit dispatch
- VirtIO backends: `virtio-blk` (VFS), `virtio-net` (Net), `virtio-console`,
  `virtio-gpu` (2D scanout → compositor)
- Linux guest: Alpine boot verified, `apk add` functional

**Required additions:**

| Gap | Description | Effort |
|---|---|---|
| `virtio-input` device model | Forward keyboard/mouse from Cellos input service → guest | 2-3 days |
| RAM budget increase | Current 128 MB → 256-512 MB for Chrome | Config change |
| Clipboard bridge | Copy/paste between host cells and guest via virtio-console channel | 1 week |
| File sharing | Share VFS paths with guest via virtio-9p or shared directory | 2 weeks |
| Browser launcher Cell | Thin Tier 1 cell: spawn hypervisor, bridge input, manage lifecycle | 1 week |

**Use cases:** Gmail, Google Docs, YouTube, GitHub, Twitter/X, Slack, Discord,
any JavaScript-heavy SPA or complex web application.

## 3. Rejected alternatives

- **Single native browser for everything** — impossible without reimplementing
  the web platform (~1,100 interfaces, ~15,000 methods/properties). Chrome is
  35M LOC / 10,000 engineer-years.
- **Servo/Blitz as native engine** — Servo requires full `std` + SpiderMonkey
  (~2M LOC). Blitz requires `std` + wgpu + winit (~50K LOC). Neither is
  portable to Cellos in any practical timeframe.
- **WebView-only (no full browser)** — G2 desktop users need Gmail/Docs/YouTube.
  Ocel cannot serve these. Tier 3 Chrome is the only path.
- **Boa instead of QuickJS** — 29x slower, mark-sweep GC causes UI jank, 6-10
  week porting effort vs 2 weeks, OOM risk on 4 MiB heap. Rust safety moot
  on Tier 2 (MMU containment). See §2.1 analysis.
- **Port tiny-skia into ViUI** — adds external dependency, different pixel
  format (RGBA premultiplied vs BGRA8888), couples ViUI to tiny-skia API.
  Referencing algorithms and implementing directly in ViCanvas is cleaner.

## 4. Consequences

- Two apps in `cells/apps/`: `ocel/` (Tier 2) and a browser launcher (Tier 1)
  that manages the Tier 3 Chrome hypervisor instance.
- Ocel becomes the default handler for `.txt`, `.md`, `.html`, `.pdf`, `.png`,
  `.svg`, `.epub` file opens from shell/desktop.
- Full web URLs (`https://gmail.com`) launch the Tier 3 Chrome instance.
  Simple/local URLs (`file:///docs/readme.md`, internal HTTP) open in Ocel.
- Ocel is the first Tier 2 application cell with Cellos `std` + `ffi-posix`
  profile, driving completion of the Tier 2 application route.
- QuickJS integration requires `setjmp`/`longjmp` and `math.h` (`libm`) in the
  POSIX shim — these additions benefit all future C FFI cells.
- ViUI rendering upgrades (AA, paths, gradients) benefit all native UI cells
  (desktop, robot-dashboard, fb-console), not just Ocel.
- MuPDF AGPL license is compatible with Cellos's open-source model. Commercial
  deployment may require MuPDF commercial license evaluation.
- The `JsEngine` trait abstraction prepares for a future engine swap (e.g., Boa)
  if Boa gains JIT/incremental GC and Cellos gains threading.

## 5. Roadmap

```
Phase 1 — Ocel Document Viewer (v0.1, ~10 weeks)
  ├── Plain text viewer (monospace, scrolling)
  ├── Markdown viewer (pulldown-cmark → HTML pipeline)
  ├── HTML/CSS subset renderer (40 tags, 60 properties)
  ├── Image viewer (PNG, BMP)
  ├── Address bar + file:// navigation
  ├── VFS integration (open files from /data, /srv)
  └── UI: tab bar, scroll, back/forward

Phase 2 — Interactive + PDF (v0.2, ~8 weeks)
  ├── QuickJS integration (basic DOM, events)
  ├── PDF viewer (MuPDF via ffi-posix)
  ├── HTTP/1.1 + basic TLS (mbedTLS)
  ├── Syntax highlighting for code files
  └── Search in document (Ctrl+F)

Phase 3 — Rich Content (v0.3, ~6 weeks)
  ├── EPUB reader (unzip + HTML pipeline)
  ├── SVG subset rendering
  ├── JPEG decoder
  └── Bookmark/history

Phase 4 — Tier 3 Chrome (parallel, ~4-6 weeks)
  ├── virtio-input device model
  ├── Browser launcher Cell
  ├── Input bridge + clipboard
  ├── RAM budget increase + Chromium smoke test
  └── File sharing (virtio-9p)
```

Phase 4 may start in parallel with Phase 1-2 since it uses existing hypervisor
infrastructure.

## 6. Cross-references

| Topic | Document |
|---|---|
| Execution tiers and admission | `docs/specs/18-cell-trust-tiers.md` |
| Tier 2 implementation gate | `docs/specs/22-native-domain-cell-implementation-gate.md` |
| Native SDK contract | `docs/specs/23-native-sdk-contract.md` |
| Application tier taxonomy | `docs/decisions/0003-application-tier-taxonomy.md` |
| Dual-mode hybrid architecture | `docs/decisions/0015-dual-mode-hybrid-architecture.md` |
| FFI/POSIX profile | `docs/specs/05-application.md` §3 |
| IPC wire contract | `docs/specs/17-ipc-wire-contract.md` |
| Hypervisor service | `cells/services/hypervisor/` |
| ViUI library | `libs/viui/` |
| Cellos std PAL | `patches/rust-std-cellos.patch` |
| Desktop environment | `cells/apps/desktop/` |

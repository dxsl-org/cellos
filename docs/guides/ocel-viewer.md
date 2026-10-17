# Ocel viewer — lightweight native document viewing

Ocel viewer is the native document viewer for Cellos. Its scope is Markdown,
source code, text, PNG/JPEG/BMP and a bounded HTML/CSS subset, with optional
JavaScript in a separate Tier 2 engine. The existing PDF service is under
footprint review; its presence is not a lightweight-profile requirement.

The product priorities are small footprint, low memory use and fast viewing,
not general browser compatibility. Controlled HTML content must stay within
the supported subset; an internal URL does not guarantee compatibility.
Full web browsers belong in Tier 3 Linux guests; Ocel does not grow a second
native browser engine. See [ADR-0017](../decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md).

## Pieces

| Piece | Path | Role |
|---|---|---|
| Viewer cell | `cells/apps/ocel/` | Window, parsers, layout, paint, address bar, tabs, search, network fetch |
| **QuickJS engine cell** | `cells/services/ocel-quickjs/` | Vendored QuickJS `2026-06-04` behind the `JsEngine` trait; parks on IPC as `service::OCEL_JS` (16) |
| Statement-matcher cell | `cells/services/ocel-js/` | Legacy standalone matcher; explicitly rejects document-backed DOM synchronization |
| DOM model + wire types | `libs/dom-arena/` | `NodeId`/`DocumentArena`, `DomMutation`, `DomEvent`, `OcelJsRequest`/`OcelJsResponse` |
| Lanes | `tests/integration/tests/ocel-browser.rs` (viewer), `…/ocel-quickjs.rs` (engine) | QEMU evidence; both wired into the `boot-suite` CI job |
| Document fixture | `tests/fixtures/ocel-js-demo.html` | Placed in VIFS1 at `/data/ocel-js-demo.html` by both image builders |

### Standalone typography build

The viewer requires the `ostd::ipc` exact-operation wrappers and the exhaustive
error mappings in `ServiceRef` and `LocalEndpoint`; the syscall numbers and
status wire format are defined in `api::syscall`. These dependencies must be
committed with the viewer rather than supplied by a dirty working tree.

From a clean checkout, use the RV64 PIC flags already used by CI:

```sh
cargo test --target x86_64-unknown-linux-gnu -p ostd -p viui -p fb-console -p ocel -p desktop
CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS="-C relocation-model=pic" \
  cargo build --release --target riscv64gc-unknown-none-elf \
  -p desktop -p fb-console -p ocel -p robot-dashboard -p viui-demo
```

This verifies application compilation and hosted contracts, not execution of
async IPC in a guest. The kernel must also implement syscalls 256–261 for
demand-engine leases to run; committing user-space wrappers alone does not
provide that kernel backend.

## How the script engine is wired (and why it is Tier 2)

1. Both engine cells declare `tier = api::manifest::PROTECTION_CLASS_UNTRUSTED`.
   The loader classifies a signed UNTRUSTED (or unsigned) artifact as a domain
   cell, so the kernel admits it to Tier 2 and binds a private page table to it
   (`kernel/src/task/launch.rs`, log line `[domain] admitted cell
   'ocel-quickjs' to Tier 2 Paged Domain (SATP isolation)`).
2. Neither engine cell is spawned during boot. `init` registers `service::OCEL_ACTIVATOR`
   (19) and listens for demand leases. An image without optional engines boots as a
   lightweight static viewer.
3. When Ocel opens a script-bearing HTML document or a PDF, it acquires an
   allowlisted lease from `service::OCEL_ACTIVATOR` over exact-operation async IPC.
   `init` derives caller identity from the kernel receive trailer, monitors client
   lifetime via `sys_notify_on_exit`, and starts the requested Tier 2 engine
   on demand (`/bin/ocel-quickjs` as `service::OCEL_JS` 16, or `/bin/ocel-pdf` as
   `service::OCEL_PDF` 18).
4. Multiple documents or tabs share the live engine instance under distinct leases.
   When a document is reset, navigated away from, or closed, the lease is released;
   when the last active lease is surrendered or an owner exits, `init` shuts down the
   unused engine (`sys_force_exit`) and unregisters it.
5. Missing or unconfigured engines produce clear, user-visible errors. Ocel never
   substitutes a matcher or guesses an engine path. Static HTML, Markdown, source code,
   plain text, and images never activate an engine.
The engine cell names itself and proves it executes at start-up, so the log says
*which* engine answered and that it is not a compiled-but-broken build:

```text
[ocel-quickjs] quickjs 2026-06-04 engine ready (Tier 2)
[ocel-quickjs] self-check 55:2,4,6:accent:0.30
```

That self-check is a loop summing 1..10, a closure mapping `[1,2,3]`, a Unicode
regexp over precomposed accents, and `(0.1 + 0.2).toFixed(2)` — the
`tests/integration/tests/ocel-quickjs.rs` lane asserts the exact string.

### The QuickJS engine cell

`cells/services/ocel-quickjs/` vendors upstream QuickJS `2026-06-04`
(<https://bellard.org/quickjs/quickjs-2026-06-04.tar.xz>, SHA-256
`b376e839…70ad2a`, MIT) as a source snapshot with its `LICENSE`, `VERSION` and a
complete deviation inventory in that directory's `README.md`. It compiles the
upstream `libquickjs` file set (`quickjs.c`, `libregexp.c`, `libunicode.c`,
`cutils.c`, `dtoa.c`) — not `quickjs-libc.c`, which is the `qjs` host's std/os
layer — and drives it from Rust over a small FFI surface.

What the port needed, and what it did **not**:

| Need | Resolution |
|---|---|
| libc (memory, strings, stdio, math) | Already exported as the Tier-A C ABI in `libs/api/src/services/posix/`; the engine links no external libc, so this cell builds on CI's bare-elf GCC, which ships no `libc.a` |
| float `printf`, `strtod` | Not needed: this release carries its own float64 parse/print (`dtoa.c`, `js_dtoa`/`js_atod`) and only formats `%u`/`%s` |
| `setjmp`/`longjmp` | Not needed either (upstream includes `<setjmp.h>` in `dtoa.c` and never jumps); Tier A exports them regardless |
| `malloc`/`free`/`realloc` | Tier A, over the cell's own heap (`ostd::declare_custom_heap!`) |
| `abs`, `lrint` | `abs` in the cell's C glue, `lrint` over `libm` in the cell's Rust shim |
| `gettimeofday`, `clock_gettime`, `localtime_r` | The cell's shim, over the cell clock; `localtime_r` reports **UTC** (no timezone database) and `Date` therefore has no local time |
| Atomics / `SharedArrayBuffer` | Compiled out (`VIOS_NO_ATOMICS`): a cell is single-threaded, and `Atomics.wait` cannot block without threads. This is the one local deviation from upstream source, marked in `vendor/quickjs/quickjs.c` |
| Headers | `vendor/include/` declares exactly the subset the engine uses — no toolchain libc headers involved |

Two size facts worth knowing before touching this cell: the engine's ELF is
~1.5 MB before `-Os` + `--strip-all` (the cell's build script sets both; the C
symbol table alone was 700 KB) and the cell's start-up line depends on a heap
declared with `ostd::declare_custom_heap!`. A cell that large cannot be read
through the VFS cell on the way to the loader — `init` therefore spawns engine
cells through the kernel loader route and the builders keep
`/bin/ocel-quickjs` in VIFS1.

### Engine capability, honestly

QuickJS executes real JavaScript against a synchronized native document,
not synthetic fixed-number nodes. `getElementById` uses connected elements'
actual `id` attributes. Node creation, append/remove, `textContent`,
attributes, title and bubbled event listeners emit ordered mutations into
the Rust arena. Existing wrappers/listeners survive same-document refreshes.
Child collections are snapshot arrays, not live browser collections.

Snapshots, scripts and replies use ordered chunks over 4 KiB copied IPC;
serialized transfers are bounded to 512 KiB. Caller/context tokens prevent
one viewer from silently executing against another viewer's DOM. A different
live caller receives Busy rather than evicting the owner. Reset/close explicitly
releases the context before its engine lease. Switching tabs invalidates the
earlier execution context; F5 reloads it. Owner traffic renews a 6,000 scheduler
tick idle lease (60 seconds); expired contexts require synchronization again.
QuickJS uses a 3 MiB accounted heap, bounded interrupt/job processing, mutation
and log budgets, and a size-counting pass before response allocation.

Native CSS now applies selector matching, importance/specificity/source-order
cascade, inheritance, inline styles, width media queries, and Taffy block,
flex and grid layout. External classic scripts and stylesheets load in
document order from the same origin (UTF-8, at most 256 KiB per resource).
`tests/fixtures/ocel-native-web.{html,css,js}` exercises these paths.

This is **not full browser compatibility**. The HTML tree builder and CSS parser
remain subsets. Native images and a bounded GET form subset use the active
Taffy renderer. Modules, fetch/XHR, timers, workers, innerHTML and complete Web
APIs are not implemented; unsupported APIs are absent rather than successful
stubs. OpenType shaping and positioned/float/table HTML layout remain unsupported.

## Typography

Ocel uses the bundled static Inter 4.1 faces for UI and document text:
Regular, Semibold, Italic and Semibold Italic. Code and monospace runs use
JetBrains Mono 2.304 Regular/Bold. These unmodified TTFs retain their SIL OFL
1.1 licenses under `libs/ostd/assets/fonts/`.

HTML uses CSS pixel sizes, while Markdown/plain text/tables use measured
pixel-width wrapping. Layout and paint share one lazy `TextFonts` context,
including baseline/line metrics, alignment and link hit rectangles.
Grayscale glyph coverage is clipped to the viewport and composited in BGRA.
Computed CSS font sizes are limited to 128px before layout and rasterization
to preserve the previous renderer's bounded size range and avoid allocating
offscreen giant glyphs.
Vietnamese precomposed and decomposed input is normalized to NFC; the old
8×8 bitmap diacritic synthesis is removed.

`GlyphAtlas` borrows bundled font bytes and rasterizes requested outlines
using `ab_glyph` rather than retaining eager geometry for every glyph.
This is not full browser typography: OpenType GPOS/GSUB shaping, bidi,
arbitrary CSS font-family loading and cross-script Noto fallback are not
implemented. Font glyph coverage alone does not provide those features.

Typography verification (2026-10-08): hosted behavior suites passed
186 tests, with 9 ignored tests not exercised; the five changed display
apps built in RV64 release mode. Actual source paint was exercised on
host pixel buffers for desktop/Ocel/console, and the default ViUI app
rendered through its real headless renderer with input and relayout.
Ocel HTML inline whitespace, Vietnamese NFC/NFD, style faces, clipping
and measured table/text layout were exercised.

A signed minimal input+UI QEMU image displayed the newly built
JetBrains Mono console through the compositor. Desktop/Ocel guest paint
was not reached: the VFS cell exhausted its heap and exited with code
238 (allocation requests of 1MiB in the full profile and 2MiB in the
minimal profile). These runs do not establish physical-display quality
or guest desktop/Ocel verification, and no VFS/grant changes were made
as part of typography.

## Using it

```text
ocel                                  # built-in welcome page
ocel file:///data/ocel-js-demo.html   # open a document by argument (argv)
```

| Action | Controls |
|---|---|
| Tabs/history | Ctrl+T, Ctrl+W, Ctrl+Tab; Alt+Left/Right or toolbar arrows |
| Navigate/reload/cancel | Enter or Go; F5 reload; Esc cancels pending loading |
| Find | Ctrl+F opens; Enter/F3 next, Shift+Enter/Shift+F3 previous; Esc closes |
| Reader | Ctrl+R or Reader; Alt+[ / Alt+] changes the reading column |
| Zoom | Ctrl++ / Ctrl+- in 10% steps, Ctrl+0 resets; range 50–200% |
| Contents | Ctrl+M; click heading or arrows/Enter; wheel scrolls the heading list |
| Scripts | Ctrl+J toggles the current tab's policy; F5 explicitly reloads |
| Scroll | Arrows, PageUp/PageDown, Home/End, wheel or scrollbar |

Reader state belongs to each document; changing tabs cannot restore another
tab's content. Native control focus clears on tab changes/navigation. Text,
checkbox and submit controls support owning-form GET actions with UTF-8 query
encoding; POST and unsupported controls report explicit errors. Ordinary
buttons do not submit. Selection/clipboard integration is not implemented.

Search is NFC-normalized and Unicode-lowercased, including Vietnamese. Matching
layout regions are highlighted; the active region has a distinct color. This
is region-based navigation, not an occurrence counter for every word on a line.
Heading contents and fragments work for Markdown and HTML.

## Loading and resource budgets

Navigation polls exact IPC operations from the input loop. It does not block
on network receive. Esc, a newer navigation, closing a tab or switching tabs
cancels the old load; a late result cannot replace the new document. The old
document/history remains until a completed candidate is accepted. Local file
reads and PDF work remain bounded synchronous operations.

HTTP validates status, bounded headers, Content-Length/chunk framing and EOF;
short or malformed responses are errors. Redirects are bounded to five hops
with loop/protocol checks; resource redirects cannot escape the original
origin. HTTP(S)/Gemini operations use a 45-second deadline; the complete
document/resource acquisition has a 60-second deadline, using monotonic time.
TLS still requires the network service's authenticated time/certificate policy.

HTML resources are discovered from active tokenized syntax; Markdown fenced
examples do not fetch images. Same-origin scripts/styles/images are prefetched
cooperatively, then parsing reads only the current tab's resource bundle.
Image bytes are shared, and relayout/Reader do not decode them again.

| Limit | Value |
|---|---|
| Open tabs / history entries per tab | 4 / 128 |
| Shared document/resource/loading residency budget | 8 MiB |
| Viewer heap | 16 MiB, including fonts and temporary working space |
| Text bytes / DOM nodes / nesting | 1 MiB / 16,384 / 64 |
| CSS rules / selectors / selector bytes | 1,024 / 4,096 / 512 |
| Resource references / total encoded resource bytes | 64 / 4 MiB |
| Script or stylesheet bytes | 256 KiB each |
| Images / decoded image bytes per document | 16 / 4 MiB |

Layout and mutation staging reserve conservative working memory before growth,
then check residency before publication. These limits are ceilings, not promises
that every smaller document fits: a 214 KiB, 4,000-node Markdown stress input is
rejected by the layout reservation. Closing tabs or using smaller documents
can be necessary. Font caches remain separately bounded by the typography SDK.

Chrome-only edits repaint/damage the toolbar, tab bar and status, leaving
document pixels intact. Scroll repaints the viewport and status, not navigation
chrome. No unbounded document/layout cache is introduced.

### Native syntax highlighting

Source files ending in `.rs`, `.c`, `.h`, `.cpp`, `.cc`, `.cxx`, `.hpp`,
`.json`, or `.toml` are displayed as code with lexical colours for keywords,
strings/characters, numbers, and comments. URL query/fragment suffixes do not
affect source-language detection. Markdown triple-backtick fences accept
`rust`/`rs`, `c`/`h`/`cpp`/`c++`/`cc`/`cxx`/`hpp`, `json`, and `toml`.

Open a source file with `ocel file:///data/example.rs`, or include a labelled
code fence in a Markdown document. Highlighting is computed during layout,
not each frame. It preserves text, indentation, and line count; Rust nested
block comments/raw strings and TOML triple-quoted strings carry state between
lines within a block. Each code block starts with fresh state.
Search joins adjacent coloured spans before matching, so a query such as
`let count =` can cross keyword/plain-text token boundaries.

This is a small native lexer, not a compiler or a full language grammar:
there is no semantic/type highlighting, C preprocessor evaluation, or syntax
validation. Unlabelled/unsupported fences keep the existing uniform code
colour; `.txt`/`.log` remain plain text. No JS engine, VM, new service, or
public ABI is needed.

Verification: eight lexer/search regressions passed in an isolated host harness.
The same harness exercised the actual source/Markdown parsers, layout,
glyph table, and pixel renderer on an in-memory surface, checking token
colours and text preservation and rendering a scrolled viewport. This is
host renderer evidence, not a QEMU compositor or physical-display claim.

## Observable behaviour

The viewer logs completed loads, searches and DOM titles. The engine identifies
itself with `[ocel-quickjs] quickjs 2026-06-04 engine ready (Tier 2)`. Error paths
report unavailable/busy engines; no matcher or in-process fallback executes
document scripts.

## Running the lane

```bash
# Build the cells and (re)assemble VIFS1 + kernel + disk_v3.img from target/
cargo build --release -p ocel -p ocel-quickjs -p ocel-pdf --target riscv64gc-unknown-none-elf
bash scripts/gen-disk-ci.sh --no-cells

cd tests/integration
CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu \
  cargo test --test ocel-browser -- --test-threads=1   # viewer plumbing
CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu \
  cargo test --test ocel-quickjs -- --test-threads=1   # engine identity + real JS
```

Raw evidence for the last passing runs: `docs/evidence/ocel-browser-qemu.log`
(viewer, serial), `docs/evidence/ocel-quickjs-qemu.log` (engine, serial) and the
matching `.txt` runner outputs.

### Upgrade verification — 2026-10-17

```sh
cargo test --offline --target x86_64-unknown-linux-gnu \
  -p ocel -p dom-arena -p ocel-service-proto -p ocel-pdf-proto -p ostd -p service-net
cargo build --offline --release --target riscv64gc-unknown-none-elf \
  -p ocel -p ocel-quickjs -p ocel-pdf -p service-net
python3 cells/services/ocel-pdf/smoke/host.py tests/fixtures/ocel-pdf-demo.pdf
```

The hosted Rust run passed 216 tests, with two existing SDK doctests ignored.
The vendored QuickJS C core executed `tests/host_dom.c` with the real prelude
and `tests/dom.js`, including mutation/log budget rejection and recovery.
MuPDF rendered both fixture pages, recovered from malformed input and rejected
an injected expired native deadline before a successful fresh open/render.

A disposable host adapter replaced only display/system boundaries; it ran the
actual viewer controller, keyboard/pointer handlers, parser, fonts, layout and
pixel painter. It exercised Reader/tab isolation and restore, form editing,
owning-form UTF-8 GET encoding, zoom, contents/anchors, Vietnamese search and
highlights, per-tab script policy, pre-submit cancellation, embedded HTML image
paint, and source/layout memory rejection preserving the old document/history.
Framebuffer captures were inspected. This is not live guest networking, a
compositor/grant test or physical-display qualification.

At a 1024×680 surface, chrome-only address editing damaged 393,216 bytes versus
2,785,280 for a full repaint, and the viewport pixels were unchanged. One warm
host sample measured roughly 0.09 ms for chrome paint and 0.71 ms for full paint;
these are scenario measurements, not hardware performance guarantees.
A 21,400-byte/400-node Markdown input used approximately 599 KiB of estimated
document residency and about 1.9 ms for layout; the larger stress input described
above was rejected before layout allocation, not timed as successful rendering.


## Image prerequisite: a display-capable `init`

The lane, like the other GUI lanes (`window-policy`, `viui-managed-surface`,
`desktop-shell`), needs the compositor and the input service, and **only `init`
can start them**: both are `#[cfg(feature = "ui")]` / `input` table rows, and
the shell holds neither a reviewed edge for `/bin/compositor` nor SpawnCap to let
a service register itself. `app-init`'s default feature set is empty
(`cells/tools/init/Cargo.toml`), so a local image assembled from a plain
`cargo build -p app-init` has no compositor and `ocel` blocks in
`wait_for_compositor()` before painting. Build `init` the way the shipping
images do:

```bash
cargo build --release -p app-init --target riscv64gc-unknown-none-elf \
  --features app-init/input,app-init/ui,app-init/ai,app-init/supervisor,\
app-init/tier3,app-init/tier3-autostart,app-init/usb-host
```

This is the same trap for every GUI lane, and the reason a "no compositor"
boot looks like a viewer bug.

## Native image viewing

Local `.png`, `.jpg`, `.jpeg` and `.bmp` files bypass the UTF-8 reader and are
decoded to straight BGRA8888. PNG and JPEG use the MIT/Apache-2.0 `zune-*`
decoders with default features disabled; BMP retains the native decoder.
PNG palette/grayscale/RGB/alpha samples are expanded to BGRA; 16-bit PNG
samples are reduced to 8-bit. Baseline and progressive JPEG are supported.
Only the first frame of an animated PNG is shown; no animation playback,
EXIF orientation transform or ICC color management is applied.

Decoding is bounded to 4096 pixels per dimension and 1,048,576 pixels total.
Encoded local images are limited to 2 MiB. Ocel initializes a 16 MiB custom
heap before any allocation; the previous default 1 MiB arena is insufficient
for ordinary decoded images. Aggregate document/tab admission can reject an
image before these per-image maxima.
Corrupt, truncated and oversized images produce a decode error rather than
being interpreted as text. Layout shares the pixel allocation with the
document, fits wide images to the viewport without upscaling, and preserves
their aspect ratio using nearest-neighbour sampling. Alpha is composited onto
the viewer background. Consecutive image nodes advance the document height.

Host smoke verification exercises the actual loader, codecs, layout and paint
with filesystem/surface adapters: RGB/RGBA/grayscale/grayscale-alpha/palette
PNG, baseline/progressive JPEG, BMP, truncation, the pixel budget and alpha
composition. The RV64 release build succeeds. This is not guest/compositor
or physical-device evidence. GIF, WebP and SVG decoding are not implemented.
Same-origin embedded HTML/Markdown images use prefetched shared resource bytes.
BMP supports BI_RGB only; BI_BITFIELDS is rejected instead of misreading masks.

## Native PDF viewing

Local `.pdf` files use the isolated Tier 2 `ocel-pdf` service (service ID 18),
not a parser inside Ocel or a Tier 3 guest. MuPDF 1.26.1 rasterizes one page
at a time, including text, vector graphics and document-embedded images.
Open `file:///data/example.pdf#page=2` for the second page; previous/next
links stay within the document's page range.

Documents are limited to 2 MiB. Each page fits within 1024 × 1024 pixels;
opaque BGRA pixels are transferred in bounded copied-IPC chunks. The viewer
closes its document handle after each load, including failure paths.
JS and PDF service ownership expires after 60 idle seconds. PDF native work has
a four-second cooperative deadline checked at allocation and result checkpoints.
Cancelling client IPC abandons its reply, not the running MuPDF invocation:
there is no hard preemption bound between checkpoints. Failed/undeliverable
results clear only the matching owner's document, and subsequent opens recover.
Encrypted documents requiring a password are rejected; password UI, text
selection, search and annotations are not implemented. Base14 fallback fonts
are embedded; CJK requires document-embedded fonts, not a bundled system
fallback. This build is RV64-only.

The service statically links AGPL-3.0-or-later MuPDF and retains upstream
source, notices and provenance. Distributing that service requires satisfying
the corresponding license obligations or obtaining a commercial MuPDF
license; the surrounding workspace's license does not replace them.

Verification: RV64 release compilation, real MuPDF host rendering of two pages
(text, vectors and an inline image), malformed-document exception recovery,
and QEMU service startup/registration. The guest viewer surface remains
unverified: the isolated smoke boot reached Ocel but its surface creation was
refused with `GrantShare … not a live private root`. No core workaround was
introduced.

## Known gaps

| Gap | Status |
|---|---|
| QuickJS integration | Real document-backed DOM; missing/incompatible engines fail explicitly |
| CSS | Native subset cascade and Taffy block/flex/grid; not standards-complete |
| PNG/JPEG/BMP | Local and same-origin embedded images; GIF/WebP/SVG and animation unsupported |
| PDF (MuPDF) | Native local-page path implemented; guest surface verification blocked as described above |
| EPUB, SVG | Not implemented |
| Syntax highlighting for text/code files | Native Rust, C/C++, JSON and TOML source files and labelled Markdown fences; other languages remain unhighlighted |
| Engine failure | Explicit errors; no in-process script fallback |
| Tier 3 Chromium browser launcher | Guest input bridge exists (`cells/services/hypervisor/src/virtio_input.rs`); launcher cell, clipboard, and file sharing do not |
| QuickJS architectures | Build predicate includes RV64, AArch64 and x86_64 when the C compiler is available; this upgrade verified RV64 release and host QuickJS, not an x86 guest |
| Multiple live JS tabs | One active context; inactive tabs require reload before event execution |

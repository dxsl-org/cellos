# Ocel viewer — lightweight native document viewing

Ocel viewer is the native document viewer for Cellos. Its scope is Markdown,
source code, text, PNG/JPEG/BMP and a bounded HTML/CSS subset, with optional
JavaScript in a separate Tier 2 engine. The existing PDF service is under
footprint review; its presence is not a lightweight-profile requirement.

The product priorities are small footprint, low memory use and fast viewing,
not general browser compatibility. Controlled HTML content must stay within
the supported subset; an internal URL does not guarantee compatibility.
A full native browser is a separate future app, with no name selected.
See [ADR-0017 §5c](../decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md#5c-ocel-viewer-scope-decision).

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
one viewer from silently executing against another viewer's DOM. There is
one active script context: loading another tab invalidates the earlier tab's
execution context; that tab must be reloaded to execute scripts again.
QuickJS uses a 3 MiB accounted heap and bounded interrupt/job processing.

Native CSS now applies selector matching, importance/specificity/source-order
cascade, inheritance, inline styles, width media queries, and Taffy block,
flex and grid layout. External classic scripts and stylesheets load in
document order from the same origin (UTF-8, at most 256 KiB per resource).
`tests/fixtures/ocel-native-web.{html,css,js}` exercises these paths.

This is **not full browser compatibility**. The HTML tree builder and CSS
parser remain subsets; modules, `fetch`/XHR, timers, workers, complete Web
APIs, browser font shaping, positioned/float/table layout, and replaced
HTML image elements are not implemented. General public-web compatibility
belongs to the separate future browser, not Ocel viewer.

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

Keys and pointer: `Ctrl+T` new tab, `Ctrl+W` close, `Ctrl+Tab` next,
`Ctrl+F`/`F3` search with `Enter` cycling matches, `Esc` closes search,
`Enter` navigates, `Backspace` edits the address bar, arrows/`PageUp`/`PageDown`/
`Home`/`End` scroll, clicking `[+]`/`[x]`/tab bodies/`[<]`/`[>]`/`[Go]` works,
links navigate, and clicks on text nodes dispatch DOM events to the Tier 2
service.

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

The viewer and the service print one line per notable event, which is what the
lane asserts on:

```text
[domain] admitted cell 'ocel-js' to Tier 2 Paged Domain (SATP isolation)
Init: ocel-js Tier 2 JS service registered.
[ocel-js] Ready to process scripts and DOM events.
[ocel] loaded file:///data/ocel-js-demo.html (HTML, 3 items)
[ocel] dom title: Tier 2 script ran
[ocel] search "cellos": 1 match(es)
[ocel] search next: 1/1
[ocel] tab 2 active: file:///welcome.md
```

## Running the lane

```bash
# Build the cells and (re)assemble VIFS1 + kernel + disk_v3.img from target/
cargo build --release -p ocel -p ocel-js -p ocel-quickjs --target riscv64gc-unknown-none-elf
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
for ordinary decoded images. These per-image limits are not a guarantee that
an unlimited number of image tabs fits in that heap.
Corrupt, truncated and oversized images produce a decode error rather than
being interpreted as text. Layout shares the pixel allocation with the
document, fits wide images to the viewport without upscaling, and preserves
their aspect ratio using nearest-neighbour sampling. Alpha is composited onto
the viewer background. Consecutive image nodes advance the document height.

Host smoke verification exercises the actual loader, codecs, layout and paint
with filesystem/surface adapters: RGB/RGBA/grayscale/grayscale-alpha/palette
PNG, baseline/progressive JPEG, BMP, truncation, the pixel budget and alpha
composition. The RV64 release build succeeds. This is not guest/compositor
or physical-device evidence. GIF, WebP and SVG decoding are not implemented;
network and embedded HTML/Markdown images are not loaded by this local-file
path.

## Native PDF viewing

Local `.pdf` files use the isolated Tier 2 `ocel-pdf` service (service ID 18),
not a parser inside Ocel or a Tier 3 guest. MuPDF 1.26.1 rasterizes one page
at a time, including text, vector graphics and document-embedded images.
Open `file:///data/example.pdf#page=2` for the second page; previous/next
links stay within the document's page range.

Documents are limited to 2 MiB. Each page fits within 1024 × 1024 pixels;
opaque BGRA pixels are transferred in bounded copied-IPC chunks. The viewer
closes its document handle after each load, including failure paths.
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
| PNG/JPEG/BMP | Local image viewing implemented; network/embedded images, GIF/WebP and animation remain unsupported |
| PDF (MuPDF) | Native local-page path implemented; guest surface verification blocked as described above |
| EPUB, SVG | Not implemented |
| Syntax highlighting for text/code files | Native Rust, C/C++, JSON and TOML source files and labelled Markdown fences; other languages remain unhighlighted |
| Engine failure | Explicit errors; no in-process script fallback |
| Tier 3 Chromium browser launcher | Guest input bridge exists (`cells/services/hypervisor/src/virtio_input.rs`); launcher cell, clipboard, and file sharing do not |
| QuickJS on x86_64 | The Tier-A C ABI the engine links against exists only on riscv64/aarch64 (`libs/api/src/services/posix.rs`), so x86_64 builds the cell as a stub that says so |
| Multiple live JS tabs | One active context; inactive tabs require reload before event execution |

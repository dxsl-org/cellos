# Ocel — native viewer and Tier 2 script service

Ocel is the native document viewer for Cellos: a Tier 2 cell that parses and
renders Markdown, an HTML subset, plain text, and BMP images, with an optional
script engine that runs in a **separate, hardware-isolated Tier 2 domain**.
It is not a web browser — [ADR-0017](../decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md)
decides that the web platform runs in the Tier 3 Linux guest (Chromium), and
Ocel covers the "open this local file / internal dashboard instantly" case.

## Pieces

| Piece | Path | Role |
|---|---|---|
| Viewer cell | `cells/apps/ocel/` | Window, parsers, layout, paint, address bar, tabs, search, network fetch |
| **QuickJS engine cell** | `cells/services/ocel-quickjs/` | Vendored QuickJS `2026-06-04` behind the `JsEngine` trait; parks on IPC as `service::OCEL_JS` (16) |
| Statement-matcher cell | `cells/services/ocel-js/` | Fallback engine for images that do not package QuickJS |
| DOM model + wire types | `libs/dom-arena/` | `NodeId`/`DocumentArena`, `DomMutation`, `DomEvent`, `OcelJsRequest`/`OcelJsResponse` |
| Lanes | `tests/integration/tests/ocel-browser.rs` (viewer), `…/ocel-quickjs.rs` (engine) | QEMU evidence; both wired into the `boot-suite` CI job |
| Document fixture | `tests/fixtures/ocel-js-demo.html` | Placed in VIFS1 at `/data/ocel-js-demo.html` by both image builders |

## How the script engine is wired (and why it is Tier 2)

1. Both engine cells declare `tier = api::manifest::PROTECTION_CLASS_UNTRUSTED`.
   The loader classifies a signed UNTRUSTED (or unsigned) artifact as a domain
   cell, so the kernel admits it to Tier 2 and binds a private page table to it
   (`kernel/src/task/launch.rs`, log line `[domain] admitted cell
   'ocel-quickjs' to Tier 2 Paged Domain (SATP isolation)`).
2. `init` spawns **one** engine cell during boot and registers the service on
   its behalf (`cells/tools/init/src/boot.rs`, `spawn_optional_services`):
   `/bin/ocel-quickjs` when the image packaged it, `/bin/ocel-js` otherwise.
   Which engine an image gets is therefore an image decision, and the boot log
   names it (`Init: registered OCEL_JS on /bin/ocel-quickjs`). The cell itself
   cannot register: ordinary `RegisterService` requires SpawnCap, which an
   untrusted cell must not hold. Both reviewed init edges are in
   `kernel/src/loader/launch_profile/profiles.rs`; each child ceiling is
   `CapSet::EMPTY` (`kernel/src/loader/boot_ceiling.rs`).
3. The viewer looks up `service::OCEL_JS` with a bounded retry
   (`cells/apps/ocel/src/js/bridge.rs`) and sends `OcelJsRequest` frames over
   4 KiB IPC, receiving replies with `ostd::ipc::recv_from` (masked to the
   service tid). A reply is applied as a `DomMutation` batch.
4. If the service is absent — not packaged, refused by the admission policy
   (production AArch64/x86_64 images refuse domains: `switch_ordering_qualified`
   is false there), or dead — the viewer falls back to its in-process engine.

Both paths are fail-soft, and the viewer says which one served the document:

```text
[ocel] js backend: Tier 2 domain service
[ocel] js backend: in-process fallback (no OCEL_JS service)
```

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

With QuickJS packaged, documents get real JavaScript: control flow, functions,
closures, objects, the standard library, `RegExp` with the Unicode tables.
Without it (or when the service is refused or dead), the viewer falls back to
`ocel-js`, a **line-oriented statement matcher** — `console.log(...)`,
`document.title = "…"`, `node_<id>.textContent = "…"`,
`node_<id>.setAttribute(...)`, `node_<id>.addEventListener(...)`, variable
assignment — with no control flow, functions or expression evaluation. Both
engines implement the same DOM dialect (`document`, `node_<id>`, `console.log`,
event dispatch), defined for QuickJS by `cells/services/ocel-quickjs/src/prelude.js`.

Both engines run the *same* fixture document
(`tests/fixtures/ocel-js-demo.html`, a loop, a closure and
`Array.prototype.map`), and the serial log shows the difference plainly:

| Image | `[ocel] dom title: …` |
|---|---|
| QuickJS packaged | `sum=10 doubled=2, 4, 6` — the computed value |
| statement matcher only | `sum=" + sum + " doubled=" + doubled` — the literal expression, because the matcher has no expression evaluation |

`boot-suite` runs the viewer lane twice for exactly this reason: once on the
image that packages QuickJS (`CELLOS_EXPECT_ENGINE_CELL=/bin/ocel-quickjs`) and
once on an image assembled with `CELLOS_NO_OCEL_QUICKJS=1`
(`CELLOS_EXPECT_ENGINE_CELL=/bin/ocel-js`), with evidence in
`docs/evidence/ocel-quickjs-qemu.log`, `…/ocel-browser-qemu.log` and
`…/ocel-js-fallback-qemu.log`.

Ocel is still **not a web browser**: no CSS cascade, no layout engine from the
web platform, no `fetch`/`XMLHttpRequest`, no modules, no workers. Documents
that need the web platform belong to the Tier 3 Chromium lane.

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

## Known gaps

| Gap | Status |
|---|---|
| QuickJS integration | **Landed** (2026-10-06) as the opt-in sibling cell `cells/services/ocel-quickjs/`; images that package it get real JavaScript, images that do not keep the statement matcher |
| CSS | No cascade/taffy: layout is the viewer's own block layout, HTML inline styles only |
| PDF (MuPDF), EPUB, SVG, PNG/JPEG | Not implemented; BMP is the only image decoder |
| Syntax highlighting for text/code files | Native Rust, C/C++, JSON and TOML source files and labelled Markdown fences; other languages remain unhighlighted |
| Viewer fallback after `ocel-js` dies mid-session | The bridge invalidates the cached tid and falls back, but no lane kills the service to witness it |
| Tier 3 Chromium browser launcher | Guest input bridge exists (`cells/services/hypervisor/src/virtio_input.rs`); launcher cell, clipboard, and file sharing do not |
| QuickJS on x86_64 | The Tier-A C ABI the engine links against exists only on riscv64/aarch64 (`libs/api/src/services/posix.rs`), so x86_64 builds the cell as a stub that says so |
| Engine death mid-session | The bridge invalidates the cached tid and falls back to the in-process engine, but no lane kills the engine cell to witness it |

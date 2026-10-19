# Ocel viewer — lightweight native document viewing

Ocel viewer is the native document viewer for Cellos. Its scope is Markdown,
source code, text, PNG/JPEG/BMP/WebP and a bounded HTML/CSS subset, with optional
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

### Explicit x86 browser-engine boot profile

The normal `policy-required` fleet posture still denies all native domains.
After preparing the embedded filesystem with signed QuickJS and init artifacts,
`bash scripts/build-cloud-images.sh --javascript` explicitly selects the browser
profile. Without `--javascript`, that builder retains its closed fleet posture.
The kernel feature is `browser-engine-profile`; it implies
`native-domains,policy-required,signing-required` and is not a default feature.
For a direct PC kernel build, use `--target x86_64-unknown-none
--features board-x86-pc,browser-engine-profile`.

The existing development signing and policy keys remain development-only.
A provisioned fleet build must instead use `--no-default-features
--features board-x86-pc,browser-engine-profile` and real kernel trust anchors;
the in-tree production key placeholders cannot verify an image and this profile
does not bypass that provisioning requirement.

Build init with `--target x86_64-unknown-none --features input,ui` for the graphical
image. There is no QuickJS init feature or eager-start switch: activation is
always demand-driven. Package signed `/bin/init`, `/bin/ocel`, and the real
signed `/bin/ocel-quickjs` in VIFS1, plus the normal signed service stack. Package
the generated `/etc/cellos/services.toml` and `/etc/cellos/autoload.toml` where VFS
serves them. Do **not** list QuickJS in either boot plan; its path and service
identity are reserved for init's fixed-target activator. A script-bearing HTML
document (for example `/data/ocel-js-demo.html`) provides activation demand.

VIFS1 must also contain `/POLICY.BIN`, signed by the configured policy root,
with an explicit `/bin/ocel-quickjs` entry whose **every capability byte is zero**.
`scripts/sign-policy.py --out POLICY.BIN` already emits that row for development
images. Retain the normal init lifecycle/spawn and platform/service/driver rows;
the empty QuickJS row does not replace them. Missing, malformed, invalidly signed
policy, an absent row, or a nonempty QuickJS row leaves browser admission closed.
No maintenance policy flag or maintenance-mode build feature is needed.

Enablement still requires the live kernel CR3 and this boot's qualified x86
substrate (including bidirectional IPI/local-invalidation proof on SMP).
Publication is additionally restricted to exact `/bin/ocel-quickjs` with an empty
capability set: signed `/bin/tier2-*` fixtures, the statement matcher, and PDF
remain denied in this profile. Unsigned artifacts are refused; present signatures
must verify. Existing manifest/owner authorization, launch-edge ceiling,
resource quota, copied IPC, live-domain-grant containment, admission lease and
drain checks remain in force. Denial never falls back to SAS or another engine.

The x86 QEMU/KVM image was exercised with `/data/ocel-native-web.html` and its
external script: the first card becomes “Script updated the real document”,
a new paragraph reads “Created by JavaScript”, and clicking **Update DOM**
changes “Waiting” to “Click updated the document”. The guest logs CR3-isolated
Tier 2 admission, demand readiness, and the real QuickJS self-check
`55:2,4,6:accent:0.30`. Script click handlers run before native form/link actions;
a submit button without a form has no native submission action.

### QEMU DEV HTTPS profile

For local testing only, the separately approved `qemu-dev-https` profile trusts
the host OS and host state directory, but still obtains fresh signed time from
approved draft-11 sources: primary `time.txryan.com:2002`, secondary `time.teax.dev:2002`,
and tertiary `rough.time.nl:2002`. It accommodates server radius up to 5 seconds
(uncertainty span up to 10 seconds), preventing false TLS rejections on network jitter.
The host companion commits a nondecreasing time floor and advancing sequence to
`~/.local/state/cellos/qemu-dev-time.sqlite3` before replying. Keep this database,
its WAL and its directory outside all guest-image/snapshot restore sets; never
delete/reset it to work around a refused time sample.

Guest/host communication is an unsigned fixed binary exchange to SLIRP's trusted
host at `10.0.2.2:18443`. The companion binds only `127.0.0.1`; do not expose it
to an untrusted host/network. Local host users are within this DEV trust boundary.
This does **not** protect whole-host rollback, compromised host software, restored
host state, or an already restored live TLS session. Every new TLS handshake
requires a new nonce-bound host exchange; a cached guest clock alone cannot
authorize it. No stale-time or alternate-source fallback exists.

Build NET as an explicit alternative to the default fleet CA flavor:

```sh
RUSTFLAGS="-C relocation-model=pic" cargo build --release -p service-net \
  --target x86_64-unknown-none --no-default-features \
  --features qemu-dev-https -Z build-std=core,alloc
python3 scripts/cellos-sign --sign target/x86_64-unknown-none/release/service-net
```

Package that signed artifact as `/bin/net` in the embedded filesystem alongside
the signed Ocel/QuickJS stack, then build and launch:

```sh
bash scripts/build-cloud-images.sh --javascript --qemu-dev-https
python3 scripts/qemu-dev-time.py
# Separate terminal:
qemu-system-x86_64 -enable-kvm -cpu host -machine q35 -m 1024M \
  -drive file=build/cellos-x86_64.qcow2,format=qcow2,if=virtio \
  -netdev user,id=net0 -device e1000,netdev=net0 \
  -vga std -display gtk,gl=off -serial file:/tmp/cellos-ocel-https.log
```

The image builder consumes the **prepared embedded filesystem**; its flag does
not rebuild/sign/package NET automatically. Both the NET and kernel opt-ins are
required. The kernel profile disables weak RNG fallback; x86 entropy uses
CPUID-gated RDSEED/RDRAND with bounded retries. Expose these CPU capabilities;
without secure entropy the profile must fail, not substitute a software seed.
NET normalizes architecture-specific monotonic ticks before applying network,
TLS and clock deadlines.

HTTPS remains end-to-end inside the guest. Public PKI uses the pinned
`webpki-roots 1.0.3` Mozilla/CCADB snapshot, WebPKI path/hostname/EKU constraints,
RustCrypto signature verification and both endpoints of the authenticated time
window. TLS 1.3 only; no online revocation checking. The default fleet/private-CA
build remains fail-closed without its own protected authenticated time.

Actual QEMU/KVM smoke rendered `https://example.com/` with JavaScript active.
`https://tuoitre.vn` passed network/TLS fetching, prefetched 23 same-site resources,
and renders the live desktop page with correct layout, positioning, multi-column news,
Vietnamese typography, and images. The previous black screen/modal occlusion from
unsupported `visibility: hidden` and overlapping geometry from viewport-resolved
`calc()` are resolved; QuickJS initializes under CR3 isolation after the x86 page-table
user-permission fix.
An invalid-host BadSSL navigation failed and preserved the prior document; the
UI's generic TLS failure does not distinguish certificate versus protocol errors.
Stopping the companion made a new HTTPS navigation fail closed; restarting it
with the same host database restored successful HTTPS without resetting state.
Hosted regressions separately reject wrong names, leaf/intermediate validity
failures at either time bound, missing trust/chain and tampered signatures.
See [ADR-0026](../decisions/0026-use-txryan-roughtime-for-dev-signed-time.md).



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
document order from the same origin (UTF-8, at most 1 MiB per resource).
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

### Reader themes and Linear integration

Reader view supports four appearance modes via `ReaderThemeMode`:
- `ReaderThemeMode::FollowSystem` (default): dynamically resolves to the desktop's active system palette (`viui::theme::SystemPalette`) at paint time via an atomic palette observer.
- `ReaderThemeMode::Light`: forces crisp light book reading surface (`#FFFFFF`).
- `ReaderThemeMode::Dark`: forces dark reading surface (`#121216`).
- `ReaderThemeMode::Paper`: warm sepia reading surface (`#F6F3EB`) with soft ink typography.

Colors are directly derived from shared ViUI `ViTheme` tokens (`reader_bg`, `reader_text`, `reader_subtext`, `reader_link`). Code blocks in Markdown and HTML documents preserve their syntax highlighting colors across theme switches while adapting container margins and backgrounds to the active palette.

Contrast ratios satisfy WCAG 2.1 compliance across all palettes:
- Normal reader text achieves $\ge 4.5:1$ contrast against reader background (Light: 17.5:1, Dark: 13.8:1, Paper: 12.1:1).
- High-contrast hyperlinks use `mode.link_color()` with guaranteed contrast $\ge 5.0:1$ against reader background (Light: 6.92:1, Dark: 5.93:1, Paper: 6.04:1).
- Themed table headers in `cells/apps/ocel/src/doc.rs` resolve palette-aware backgrounds (`#EBEDF2` Light, `#262634` Dark, `#E8E4DA` Paper) and remapped text colors, achieving contrast $\ge 11:1$ (Light: 15.64:1, Dark: 13.71:1, Paper: 11.16:1).
- Original source color spaces are preserved for image rasters, PDF pages, and syntax highlighting on code spans.

### Native session tabs and window title synchronization

Under ADR-0027 Phase 06, Ocel's private in-app tab strip is cut over (`TABBAR_HEIGHT = 0`), allocating the full 28 DIP vertical space to the active document viewport (`height - 68` instead of `height - 96` DIP) and eliminating redundant tab strip chrome. Active document titles synchronize dynamically with the compositor window frame title (`self.surface.set_title(&self.current_tab().title)`) across `new_tab`, `switch_tab`, `close_tab`, and `finish_load`.

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
examples do not fetch images. Same-site scripts/styles/images are prefetched
cooperatively (supporting same-origin and same-site subdomains such as `*.tuoitre.vn`
for `tuoitre.vn`), then parsing reads only the current tab's resource bundle.
Cross-site resources and insecure mixed content (HTTP on HTTPS) remain blocked.
Image bytes are shared, and relayout/Reader do not decode them again.

| Limit | Value |
|---|---|
| Open tabs / history entries per tab | 4 / 128 |
| Shared document/resource/loading residency budget | 16 MiB |
| Viewer heap | 24 MiB, including fonts and temporary working space |
| Text bytes / DOM nodes / nesting | 2 MiB / 16,384 / 64 |
| CSS rules / selectors / selector bytes | 8,192 / 32,768 / 1,024 |
| Resource references / total encoded resource bytes | 256 / 8 MiB |
| Script or stylesheet bytes | 1 MiB each |
| Images / decoded image bytes per document | 128 / 8 MiB |
| Encoded bytes / pixels per image | 4 MiB / 2,097,152 |

Layout and mutation staging reserve conservative working memory before growth,
then check residency before publication. These limits are ceilings, not promises
that every smaller document fits. Closing tabs or using smaller documents can
be necessary. Font caches remain separately bounded by the typography SDK.
The 24 MiB heap leaves room for code/data/stack inside the kernel's 32 MiB PIE
slot; raising the heap beyond that requires a loader/address-space change.

The ordinary-site limits admit IANA's observed 1,211-rule, 121,482-byte stylesheet;
QEMU/KVM rendered `https://www.iana.org/` (23 document items). Under the desktop
profile, `https://tuoitre.vn/` prefetches 23 same-site resources from `*.tuoitre.vn`
(e.g. `static-tuoitre.tuoitre.vn` and `cdn2.tuoitre.vn`) and renders 363 items.
The desktop profile includes a compact `no_std` flat-path SVG rasterizer supporting
viewBox, `path` (M/L/H/V/C/Z with cubic Bézier flattening), `rect`, and `circle`;
the embedded profile keeps SVG disabled to conserve memory.

Chrome-only edits repaint/damage the toolbar and status, leaving
document pixels intact (the private tab strip is cut over to compositor native tabs with `TABBAR_HEIGHT = 0`). Scroll repaints the viewport and status, not navigation
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
PNG, baseline/progressive JPEG, BMP, WebP (VP8/VP8L/VP8X), desktop SVG, truncation,
the pixel budget and alpha composition. The RV64/x86_64 release builds succeed.
GIF and animated playback are not implemented.
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
| CSS | Native subset cascade with gradient background fallback and Taffy block/flex/grid; not standards-complete |
| PNG/JPEG/BMP/WebP | Local and same-site embedded images; WebP (VP8 lossy, VP8L lossless, VP8X extended) decoded into BGRA |
| PDF (MuPDF) | Native local-page path implemented; guest surface verification blocked as described above |
| SVG | Compact desktop-profile flat-path vector rasterizer; renders vector logos |
| Syntax highlighting for text/code files | Native Rust, C/C++, JSON and TOML source files and labelled Markdown fences; other languages remain unhighlighted |
| Engine failure | Explicit errors; no in-process script fallback |
| Tier 3 Chromium browser launcher | Guest input bridge exists (`cells/services/hypervisor/src/virtio_input.rs`); launcher cell, clipboard, and file sharing do not |
| QuickJS architectures | Build predicate includes RV64, AArch64 and x86_64 when the C compiler is available; RV64 release, host QuickJS and x86 QEMU/KVM DOM/script/click execution verified |
| Multiple live JS tabs | One active context; inactive tabs require reload before event execution |

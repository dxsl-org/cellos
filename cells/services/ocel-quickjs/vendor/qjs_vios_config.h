/* Cellos build configuration for the vendored QuickJS sources.
 *
 * Force-included (`-include`) into every vendored translation unit, so it can
 * only add or neutralise *macros* — the source tree carries exactly one
 * deviation (the `VIOS_NO_ATOMICS` guard in quickjs.c, see
 * cells/services/ocel-quickjs/README.md).
 */
#ifndef QJS_VIOS_CONFIG_H
#define QJS_VIOS_CONFIG_H

/* `js_def_malloc_usable_size` calls `malloc_usable_size` in its final `#else`
 * branch (glibc/macOS/Windows/Emscripten are the other four). The Tier-A C ABI
 * has no such symbol, and upstream's own comment on that branch invites
 * changing it to `return 0;`. Mapping it to 0 here is the same value the
 * Emscripten branch returns, without editing the source: the number only feeds
 * the runtime's allocation accounting, and a 0 usable-size makes the counter
 * track requested sizes exactly. */
#define malloc_usable_size(ptr) 0

/* The cell is single-threaded; see the deviation note in vendor/quickjs.c. */
#define VIOS_NO_ATOMICS 1

#endif

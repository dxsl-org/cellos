/* Cellos glue for the vendored QuickJS.
 *
 * `quickjs.h` exposes most value predicates and `JS_FreeValue` as `static
 * inline` functions, and the `JSValue` layout is conditional on
 * `JS_NAN_BOXING`/`CONFIG_CHECK_JSVALUE`. Rather than mirroring that layout and
 * the tag table in Rust — two copies of an ABI that upstream is free to
 * change — the driver calls these thin exported wrappers and lets the C
 * compiler do the inline expansion for the configuration this cell builds.
 *
 * `abs` is here rather than in `stdlib.h` because upstream calls it as a real
 * function: the Tier-A C ABI exports the libm-style math symbols but not the
 * integer `abs`, and a macro would not give the linker a symbol.
 */
#include "quickjs.h"

int vios_js_is_exception(JSValue v) { return JS_IsException(v); }
int vios_js_is_undefined(JSValue v) { return JS_IsUndefined(v); }
int vios_js_is_null(JSValue v) { return JS_IsNull(v); }
int vios_js_is_bool(JSValue v) { return JS_IsBool(v); }
int vios_js_is_number(JSValue v) { return JS_IsNumber(v); }

/* Float64 vs. small-int, so the driver can read the union without guessing
   from the decimal formatting. */
int vios_js_is_float(JSValue v) {
    return JS_TAG_IS_FLOAT64(JS_VALUE_GET_TAG(v));
}

double vios_js_as_float(JSValue v) { return JS_VALUE_GET_FLOAT64(v); }

void vios_js_free(JSContext *ctx, JSValue v) { JS_FreeValue(ctx, v); }

int abs(int v) { return v < 0 ? -v : v; }

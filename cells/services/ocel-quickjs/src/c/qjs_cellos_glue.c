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

/* Bound UTF-8 conversion BEFORE QuickJS allocates a C string. A UTF-16 code
 * unit needs at most three UTF-8 bytes (surrogate pairs use four for two).
 * This deliberately conservative bound also covers lone surrogate repair. */
const char *vios_js_to_bounded_string(JSContext *ctx, JSValue value, size_t limit, size_t *length)
{
    JSValue text = JS_ToString(ctx, value);
    if (JS_IsException(text)) return NULL;
    JSValue size = JS_GetPropertyStr(ctx, text, "length");
    int64_t units = 0;
    int status = JS_ToInt64(ctx, &units, size);
    JS_FreeValue(ctx, size);
    if (status < 0 || units < 0 || (uint64_t)units > limit / 3) {
        JS_FreeValue(ctx, text);
        *length = limit + 1;
        return NULL;
    }
    const char *result = JS_ToCStringLen2(ctx, length, text, 0);
    JS_FreeValue(ctx, text);
    return result;
}

int abs(int v) { return v < 0 ? -v : v; }

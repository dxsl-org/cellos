/* SPDX-License-Identifier: MIT
 * Minimal host for the real vendored QuickJS core; no quickjs-libc dependency.
 * Arguments: src/prelude.js tests/dom.js
 */
#include <stdio.h>
#include <stdlib.h>
#include "quickjs.h"

static int evaluate_file(JSContext *ctx, const char *path) {
    FILE *file = fopen(path, "rb");
    if (!file) { perror(path); return 1; }
    if (fseek(file, 0, SEEK_END) != 0) { fclose(file); return 1; }
    long size = ftell(file);
    if (size < 0 || fseek(file, 0, SEEK_SET) != 0) { fclose(file); return 1; }
    char *source = malloc((size_t)size + 1);
    if (!source) { fclose(file); return 1; }
    if (fread(source, 1, (size_t)size, file) != (size_t)size) {
        free(source); fclose(file); return 1;
    }
    fclose(file);
    source[size] = 0;
    JSValue result = JS_Eval(ctx, source, (size_t)size, path, JS_EVAL_TYPE_GLOBAL);
    free(source);
    int failed = JS_IsException(result);
    if (failed) {
        JSValue error = JS_GetException(ctx);
        const char *message = JS_ToCString(ctx, error);
        fprintf(stderr, "%s: %s\n", path, message ? message : "unprintable exception");
        if (message) JS_FreeCString(ctx, message);
        JS_FreeValue(ctx, error);
    }
    JS_FreeValue(ctx, result);
    return failed;
}

int main(int argc, char **argv) {
    if (argc != 3) { fprintf(stderr, "usage: host_dom prelude.js dom.js\n"); return 2; }
    JSRuntime *runtime = JS_NewRuntime();
    if (!runtime) return 1;
    JS_SetMemoryLimit(runtime, 3 * 1024 * 1024);
    JS_SetMaxStackSize(runtime, 192 * 1024);
    JSContext *context = JS_NewContext(runtime);
    if (!context) { JS_FreeRuntime(runtime); return 1; }
    int failed = evaluate_file(context, argv[1]) || evaluate_file(context, argv[2]);
    JS_FreeContext(context);
    JS_FreeRuntime(runtime);
    if (!failed) puts("QuickJS document DOM regressions passed");
    return failed;
}

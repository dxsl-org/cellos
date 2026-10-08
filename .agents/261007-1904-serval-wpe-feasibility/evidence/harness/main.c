#include <wpe/wpe.h>
#include <wpe/webkit.h>
#include <wpe/wpe-platform.h>
#include <stdlib.h>
#include <stdio.h>

static void load_changed_cb(WebKitWebView *web_view, WebKitLoadEvent load_event, gpointer user_data) {
    if (load_event == WEBKIT_LOAD_FINISHED) {
        printf("Load finished.\n");
        GMainLoop *loop = (GMainLoop *)user_data;
        g_main_loop_quit(loop);
    }
}

int main(int argc, char *argv[]) {
    if (argc < 2) {
        printf("Usage: %s <url>\n", argv[0]);
        return 1;
    }
    
    // Explicitly initialize WPE platform to headless
    wpe_platform_initialize();

    WebKitWebContext *context = webkit_web_context_get_default();
    WebKitWebView *view = WEBKIT_WEB_VIEW(webkit_web_view_new_with_context(context));
    
    GMainLoop *loop = g_main_loop_new(NULL, FALSE);
    g_signal_connect(view, "load-changed", G_CALLBACK(load_changed_cb), loop);
    
    webkit_web_view_load_uri(view, argv[1]);
    
    g_main_loop_run(loop);
    
    // In a real harness, we'd wait a bit for rendering/snapshot here.
    return 0;
}

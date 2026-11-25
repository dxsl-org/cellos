/*
 * Preloaded into cage only. The clipboard agent (wl-paste --watch, wl-copy)
 * reads and sets the selection through the wlr-data-control protocol, which
 * needs no focused surface of its own; cage 0.2.0 does not create that global,
 * while the wlroots it links does. Wherever cage creates the regular clipboard
 * (wl_data_device_manager), the data-control manager is created beside it on
 * the same display.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>

struct wl_display;
struct wlr_data_device_manager;

struct wlr_data_device_manager *wlr_data_device_manager_create(struct wl_display *display)
{
    static struct wlr_data_device_manager *(*real)(struct wl_display *);
    void *(*data_control)(struct wl_display *);
    if (!real)
        real = (struct wlr_data_device_manager * (*)(struct wl_display *))
            dlsym(RTLD_NEXT, "wlr_data_device_manager_create");
    struct wlr_data_device_manager *manager = real(display);
    data_control = (void *(*)(struct wl_display *))
        dlsym(RTLD_DEFAULT, "wlr_data_control_manager_v1_create");
    if (!data_control || !data_control(display))
        fputs("chrome-guest: no wlr-data-control manager\n", stderr);
    return manager;
}

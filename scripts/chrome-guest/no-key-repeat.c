/*
 * Preloaded into cage only. CellOS's input service is the one owner of key
 * repeat; the Chrome launcher turns each host repeat into a release and press.
 * A second repeat timer inside the guest counts from when a press reaches
 * Chromium, so a release delayed anywhere on its way (host IPC, a vCPU busy
 * loading Chromium) repeats the key ("httppppps://"). cage 0.2.0 hard-codes
 * 25/s after 600 ms; rate 0 makes wl_keyboard.repeat_info tell clients not to
 * repeat at all.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdint.h>

struct wlr_keyboard;

void wlr_keyboard_set_repeat_info(struct wlr_keyboard *keyboard, int32_t rate, int32_t delay)
{
    static void (*real)(struct wlr_keyboard *, int32_t, int32_t);
    (void)rate;
    (void)delay;
    if (!real)
        real = (void (*)(struct wlr_keyboard *, int32_t, int32_t))
            dlsym(RTLD_NEXT, "wlr_keyboard_set_repeat_info");
    real(keyboard, 0, 0);
}

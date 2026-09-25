/* SPDX-License-Identifier: MPL-2.0 */
/*
 * Stable application-owned hook boundary for C ports.
 *
 * A Cellos Rust host implements these hooks; C game/application code owns its
 * event loop and never receives ambient kernel or service handles.  The host
 * must keep the returned surface pointer valid until cellos_surface_destroy.
 */
#ifndef CELLOS_PLATFORM_H
#define CELLOS_PLATFORM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct cellos_surface {
    uint32_t *pixels;   /* BGRA8888, tightly packed rows */
    uint32_t width;
    uint32_t height;
    uint32_t stride;    /* bytes */
} cellos_surface;

typedef enum cellos_input_kind {
    CELLOS_INPUT_NONE = 0,
    CELLOS_INPUT_KEY_DOWN,
    CELLOS_INPUT_KEY_UP,
    CELLOS_INPUT_POINTER_MOVE,
    CELLOS_INPUT_POINTER_BUTTON,
} cellos_input_kind;

typedef struct cellos_input_event {
    cellos_input_kind kind;
    uint32_t code;
    int32_t x;
    int32_t y;
} cellos_input_event;

/* Surface and input. A host must create/focus the surface before calling C. */
int cellos_surface_create(uint32_t width, uint32_t height, cellos_surface *out);
void cellos_surface_present(const cellos_surface *surface);
int cellos_input_poll(cellos_input_event *out);

/* Monotonic time and scheduler-friendly waiting. */
uint64_t cellos_time_ms(void);
void cellos_sleep_ms(uint32_t ms);

/* VFS and TCP are ordinary declared-capability services, not path authority. */
int cellos_vfs_read(const char *path, void *buf, size_t capacity, size_t *out_len);
int cellos_vfs_write(const char *path, const void *buf, size_t len);
int cellos_tcp_connect(uint32_t ipv4_be, uint16_t port_be);

#ifdef __cplusplus
}
#endif
#endif

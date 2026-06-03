/*
 * ViOS MicroPython `vfs` module.
 *
 * Exposes VFS filesystem IPC to Python scripts via:
 *   vfs.read(path)             -> str | None
 *   vfs.write(path, content)   -> bool
 *   vfs.append(path, content)  -> bool
 *   vfs.mkdir(path)            -> bool
 *
 * Wire format mirrors bindings_vfs.rs in the Lua cell.  All IPC goes to
 * the VFS service cell at endpoint 3.
 *
 * sys_recv returns the SENDER task ID, not a byte count.  Reply length is
 * recovered by zero-scanning a pre-zeroed buffer (done inside vios_net_recv).
 * Content larger than MAX_CHUNK is split into write + append chunks.
 */

#include <stdint.h>
#include <stddef.h>
#include <string.h>

#include "py/obj.h"
#include "py/runtime.h"

/* ── ViOS syscall bridge (net_bridge.rs) ─────────────────────────────────── */
/* vios_net_send/recv are general-purpose IPC; `net` in the name is historical. */
extern void     vios_net_send(size_t endpoint, const uint8_t *buf, size_t len);
extern intptr_t vios_net_recv(size_t from_id, uint8_t *buf, size_t buf_len);

/* ── Constants ───────────────────────────────────────────────────────────── */
#define VFS_ENDPOINT  3u
#define OP_WRITE      0x04u
#define OP_MKDIR      0x05u
#define OP_READ       0x08u
#define OP_APPEND     0x0Au
#define MAX_PATH      253u
/* Conservative per-IPC content cap matching bindings_vfs.rs: 512 - 4 - path_len */
#define MAX_CHUNK     480u
/* Static receive buffer for OP_READ replies.  MicroPython is single-threaded. */
#define READ_BUF_SIZE 4096u

/* ── Helpers ─────────────────────────────────────────────────────────────── */

static void write_le16(uint8_t *b, uint16_t v) {
    b[0] = (uint8_t)v; b[1] = (uint8_t)(v >> 8);
}

/* Scan buf[0..n] for last non-zero byte. Returns byte count. */
static size_t zero_scan(const uint8_t *buf, size_t n) {
    while (n > 0 && buf[n - 1] == 0) n--;
    return n;
}

/*
 * Send one OP_WRITE / OP_APPEND IPC chunk to the VFS cell.
 * Returns 1 on success (VFS reply byte == 0).
 *
 * Wire: [opcode:1][path_len:1][content_len:2 LE][path:path_len][content:cl]
 */
static int vfs_write_chunk(uint8_t opcode,
                           const char *path, size_t pl,
                           const char *data, size_t cl)
{
    /* cl is already clamped by the caller. */
    size_t msg_len = 4u + pl + cl;
    /* Stack-allocate; msg_len <= 4 + 253 + 480 = 737 bytes. */
    uint8_t msg[4 + MAX_PATH + MAX_CHUNK];
    msg[0] = opcode;
    msg[1] = (uint8_t)pl;
    write_le16(&msg[2], (uint16_t)cl);
    memcpy(&msg[4],      path, pl);
    memcpy(&msg[4 + pl], data, cl);
    vios_net_send(VFS_ENDPOINT, msg, msg_len);
    uint8_t r[1] = {0};
    vios_net_recv(0, r, 1);
    return r[0] == 0;
}

/* Shared write/append driver. first_op = OP_WRITE (truncate) or OP_APPEND. */
static mp_obj_t vfs_write_impl(mp_obj_t path_obj, mp_obj_t data_obj, uint8_t first_op)
{
    size_t pl_raw, dl;
    const char *path = mp_obj_str_get_data(path_obj, &pl_raw);
    const char *data = mp_obj_str_get_data(data_obj, &dl);
    size_t pl = pl_raw < MAX_PATH ? pl_raw : MAX_PATH;
    /* max_chunk shrinks as path grows; guard with max(1) against 0 */
    size_t max_chunk = MAX_CHUNK > pl ? MAX_CHUNK - pl : 1u;

    size_t first_len = dl < max_chunk ? dl : max_chunk;
    int ok = vfs_write_chunk(first_op, path, pl, data, first_len);
    size_t offset = first_len;
    while (ok && offset < dl) {
        size_t end = offset + max_chunk;
        if (end > dl) end = dl;
        ok = vfs_write_chunk(OP_APPEND, path, pl, data + offset, end - offset);
        offset = end;
    }
    return ok ? mp_const_true : mp_const_false;
}

/* ── vfs.read(path) -> str | None ────────────────────────────────────────── */
/*
 * Send OP_READ, wait for reply, return file content as a Python str.
 * Returns None on empty reply (file not found or empty file).
 */
static uint8_t s_read_buf[READ_BUF_SIZE]; /* static: no stack overflow */

static mp_obj_t vfs_read(mp_obj_t path_obj) {
    size_t pl_raw;
    const char *path = mp_obj_str_get_data(path_obj, &pl_raw);
    size_t pl = pl_raw < MAX_PATH ? pl_raw : MAX_PATH;

    uint8_t req[2 + MAX_PATH];
    req[0] = OP_READ;
    req[1] = (uint8_t)pl;
    memcpy(&req[2], path, pl);
    vios_net_send(VFS_ENDPOINT, req, 2 + pl);

    memset(s_read_buf, 0, READ_BUF_SIZE);
    if (vios_net_recv(0, s_read_buf, READ_BUF_SIZE) < 0) return mp_const_none;
    size_t n = zero_scan(s_read_buf, READ_BUF_SIZE);
    if (n == 0) return mp_const_none;
    return mp_obj_new_str((const char *)s_read_buf, n);
}
static MP_DEFINE_CONST_FUN_OBJ_1(vfs_read_obj, vfs_read);

/* ── vfs.write(path, content) -> bool ───────────────────────────────────── */
static mp_obj_t vfs_write(mp_obj_t path_obj, mp_obj_t data_obj) {
    return vfs_write_impl(path_obj, data_obj, OP_WRITE);
}
static MP_DEFINE_CONST_FUN_OBJ_2(vfs_write_obj, vfs_write);

/* ── vfs.append(path, content) -> bool ──────────────────────────────────── */
static mp_obj_t vfs_append(mp_obj_t path_obj, mp_obj_t data_obj) {
    return vfs_write_impl(path_obj, data_obj, OP_APPEND);
}
static MP_DEFINE_CONST_FUN_OBJ_2(vfs_append_obj, vfs_append);

/* ── vfs.mkdir(path) -> bool ─────────────────────────────────────────────── */
static mp_obj_t vfs_mkdir(mp_obj_t path_obj) {
    size_t pl_raw;
    const char *path = mp_obj_str_get_data(path_obj, &pl_raw);
    size_t pl = pl_raw < MAX_PATH ? pl_raw : MAX_PATH;

    uint8_t req[2 + MAX_PATH];
    req[0] = OP_MKDIR;
    req[1] = (uint8_t)pl;
    memcpy(&req[2], path, pl);
    vios_net_send(VFS_ENDPOINT, req, 2 + pl);

    uint8_t r[1] = {0};
    vios_net_recv(0, r, 1);
    return r[0] == 0 ? mp_const_true : mp_const_false;
}
static MP_DEFINE_CONST_FUN_OBJ_1(vfs_mkdir_obj, vfs_mkdir);

/* ── Module table ────────────────────────────────────────────────────────── */
static const mp_rom_map_elem_t vfs_module_globals_table[] = {
    { MP_ROM_QSTR(MP_QSTR___name__), MP_ROM_QSTR(MP_QSTR_vfs) },
    { MP_ROM_QSTR(MP_QSTR_read),     MP_ROM_PTR(&vfs_read_obj) },
    { MP_ROM_QSTR(MP_QSTR_write),    MP_ROM_PTR(&vfs_write_obj) },
    { MP_ROM_QSTR(MP_QSTR_append),   MP_ROM_PTR(&vfs_append_obj) },
    { MP_ROM_QSTR(MP_QSTR_mkdir),    MP_ROM_PTR(&vfs_mkdir_obj) },
};
static MP_DEFINE_CONST_DICT(vfs_module_globals, vfs_module_globals_table);

const mp_obj_module_t mp_module_vfs_vios = {
    .base    = { &mp_type_module },
    .globals = (mp_obj_dict_t *)&vfs_module_globals,
};

/* Register as the built-in module `vfs`. */
MP_REGISTER_MODULE(MP_QSTR_vfs, mp_module_vfs_vios);

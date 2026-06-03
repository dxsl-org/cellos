/*
 * ViOS MicroPython `vnet` UDP socket operations.
 *
 * vnet.udp_socket()               -> cap_int | None
 * vnet.bind(cap, port)            -> assigned_port | None
 * vnet.udp_send(cap, ip, port, d) -> bytes_sent
 * vnet.udp_recv(cap)              -> (src_ip, src_port, data) | None
 *
 * Wire format mirrors bindings_net.rs in the Lua cell.
 * Shared bridge: vios_net_send / vios_net_recv (net_bridge.rs).
 * Helpers (parse_ipv4_udp, read_le64_udp, write_le64_udp, write_le16_udp,
 * zero_scan_udp, format_ip_buf_udp) are file-local copies to keep this file
 * self-contained without a shared header.
 */

#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include "py/obj.h"
#include "py/runtime.h"

extern void     vios_net_send(size_t endpoint, const uint8_t *buf, size_t len);
extern intptr_t vios_net_recv(size_t from_id, uint8_t *buf, size_t buf_len);

#define NET_ENDPOINT  6u
#define SOCKET_UDP    0x11u
#define CLOSE_OP      0x15u
#define BIND_OP       0x16u
#define SENDTO_OP     0x21u
#define RECVFROM_OP   0x22u
#define MAX_UDP_SEND  480u
#define POLL_RETRIES  500

/* ── Local helpers ───────────────────────────────────────────────────────── */

static uint64_t read_le64_udp(const uint8_t *b) {
    return (uint64_t)b[0] | ((uint64_t)b[1]<<8) | ((uint64_t)b[2]<<16) |
           ((uint64_t)b[3]<<24) | ((uint64_t)b[4]<<32) | ((uint64_t)b[5]<<40) |
           ((uint64_t)b[6]<<48) | ((uint64_t)b[7]<<56);
}
static void write_le64_udp(uint8_t *b, uint64_t v) {
    b[0]=(uint8_t)v;       b[1]=(uint8_t)(v>>8);
    b[2]=(uint8_t)(v>>16); b[3]=(uint8_t)(v>>24);
    b[4]=(uint8_t)(v>>32); b[5]=(uint8_t)(v>>40);
    b[6]=(uint8_t)(v>>48); b[7]=(uint8_t)(v>>56);
}
static void write_le16_udp(uint8_t *b, uint16_t v) {
    b[0]=(uint8_t)v; b[1]=(uint8_t)(v>>8);
}
static int parse_ipv4_udp(const char *s, size_t slen, uint8_t out[4]) {
    unsigned int parts[4]; int pi=0; unsigned int cur=0; int digits=0;
    size_t i;
    for (i=0; i<=slen && pi<4; i++) {
        char c = (i < slen) ? s[i] : '.';
        if (c=='.') { if (!digits || cur>255) return 0; parts[pi++]=cur; cur=0; digits=0; }
        else if (c>='0'&&c<='9') { cur=cur*10u+(unsigned)(c-'0'); digits++; }
        else return 0;
    }
    if (pi!=4) return 0;
    out[0]=(uint8_t)parts[0]; out[1]=(uint8_t)parts[1];
    out[2]=(uint8_t)parts[2]; out[3]=(uint8_t)parts[3];
    return 1;
}
/* Format 4-byte IP into buf (no NUL). Returns byte count. */
static int format_ip_buf_udp(uint8_t ip[4], char *buf) {
    int n=0, oi;
    for (oi=0; oi<4; oi++) {
        if (oi>0) buf[n++]='.';
        unsigned v=ip[oi];
        if (v>=100) { buf[n++]=(char)('0'+v/100); v%=100; buf[n++]=(char)('0'+v/10); }
        else if (v>=10) buf[n++]=(char)('0'+v/10);
        buf[n++]=(char)('0'+v%10);
    }
    return n;
}

/* ── vnet.udp_socket() -> cap | None ─────────────────────────────────────── */
mp_obj_t vnet_udp_socket(void) {
    uint8_t msg[9] = {SOCKET_UDP, 0,0,0,0,0,0,0,0};
    vios_net_send(NET_ENDPOINT, msg, 9);
    uint8_t r[8]; memset(r, 0, 8);
    if (vios_net_recv(0, r, 8) < 0) return mp_const_none;
    uint64_t cap = read_le64_udp(r);
    return cap ? mp_obj_new_int((mp_int_t)cap) : mp_const_none;
}
MP_DEFINE_CONST_FUN_OBJ_0(vnet_udp_socket_obj, vnet_udp_socket);

/* ── vnet.bind(cap, port) -> assigned_port | None ────────────────────────── */
/* port=0 → kernel assigns an ephemeral port. */
mp_obj_t vnet_bind(mp_obj_t cap_obj, mp_obj_t port_obj) {
    uint64_t cap = (uint64_t)mp_obj_get_int(cap_obj);
    uint16_t port = (uint16_t)mp_obj_get_int(port_obj);
    uint8_t msg[11]; memset(msg, 0, 11);
    msg[0] = BIND_OP;
    write_le64_udp(&msg[1], cap);
    write_le16_udp(&msg[9], port);
    vios_net_send(NET_ENDPOINT, msg, 11);
    uint8_t r[2]; memset(r, 0, 2);
    if (vios_net_recv(0, r, 2) < 0) return mp_const_none;
    uint16_t assigned = (uint16_t)r[0] | ((uint16_t)r[1] << 8);
    return (assigned == 0xFFFFu) ? mp_const_none : mp_obj_new_int(assigned);
}
MP_DEFINE_CONST_FUN_OBJ_2(vnet_bind_obj, vnet_bind);

/* ── vnet.udp_send(cap, ip, port, data) -> bytes_sent ────────────────────── */
/* 4-argument functions use VAR_BETWEEN since MP_DEFINE_CONST_FUN_OBJ_4 does
 * not exist in MicroPython's API. */
static mp_obj_t vnet_udp_send_impl(size_t n_args, const mp_obj_t *args) {
    (void)n_args; /* guaranteed 4 by VAR_BETWEEN */
    uint64_t cap = (uint64_t)mp_obj_get_int(args[0]);
    size_t ilen; const char *ips = mp_obj_str_get_data(args[1], &ilen);
    uint8_t ip[4];
    if (!parse_ipv4_udp(ips, ilen, ip)) return mp_obj_new_int(0);
    uint16_t port = (uint16_t)mp_obj_get_int(args[2]);
    size_t dlen; const char *data = mp_obj_str_get_data(args[3], &dlen);
    if (dlen > MAX_UDP_SEND) dlen = MAX_UDP_SEND;

    /* [SENDTO][cap:8][ip:4][port:2 LE][data:dlen] — min 15 bytes for zero-scan */
    uint8_t msg[9 + 6 + MAX_UDP_SEND]; memset(msg, 0, sizeof(msg));
    msg[0] = SENDTO_OP;
    write_le64_udp(&msg[1], cap);
    msg[9]=ip[0]; msg[10]=ip[1]; msg[11]=ip[2]; msg[12]=ip[3];
    write_le16_udp(&msg[13], port);
    memcpy(&msg[15], data, dlen);
    size_t mlen = 15 + dlen;
    if (mlen < 15) mlen = 15; /* floor for zero-scan */

    size_t sent = 0;
    int i;
    for (i=0; i<POLL_RETRIES; i++) {
        vios_net_send(NET_ENDPOINT, msg, mlen);
        uint8_t cnt[4]; memset(cnt, 0, 4);
        if (vios_net_recv(0, cnt, 4) < 0) break;
        uint32_t n = (uint32_t)cnt[0] | ((uint32_t)cnt[1]<<8) |
                     ((uint32_t)cnt[2]<<16) | ((uint32_t)cnt[3]<<24);
        if (n > 0) { sent = n; break; }
        extern void vios_net_yield(void);
        vios_net_yield();
    }
    return mp_obj_new_int((mp_int_t)sent);
}
MP_DEFINE_CONST_FUN_OBJ_VAR_BETWEEN(vnet_udp_send_obj, 4, 4, vnet_udp_send_impl);

/* ── vnet.udp_recv(cap) -> (src_ip_str, src_port, data_str) | None ─────── */
mp_obj_t vnet_udp_recv(mp_obj_t cap_obj) {
    uint64_t cap = (uint64_t)mp_obj_get_int(cap_obj);
    uint8_t req[13]; memset(req, 0, 13);
    req[0] = RECVFROM_OP;
    write_le64_udp(&req[1], cap);
    /* buf_len = 512 */
    req[9]=0; req[10]=2; req[11]=0; req[12]=0; /* 512 LE */

    static uint8_t rbuf[6 + 512]; /* 6-byte header + payload */
    int i;
    for (i=0; i<POLL_RETRIES; i++) {
        memset(rbuf, 0, sizeof(rbuf));
        vios_net_send(NET_ENDPOINT, req, 13);
        if (vios_net_recv(0, rbuf, sizeof(rbuf)) >= 0 && rbuf[0] != 0) {
            uint8_t sip[4] = {rbuf[0],rbuf[1],rbuf[2],rbuf[3]};
            uint16_t sp = (uint16_t)rbuf[4] | ((uint16_t)rbuf[5]<<8);
            /* find data end by NUL scan starting at byte 6 */
            size_t di = 6, end = sizeof(rbuf);
            while (end > di && rbuf[end-1] == 0) end--;
            size_t dlen = end > di ? end - di : 0;
            char ip_str[16]; int ip_len = format_ip_buf_udp(sip, ip_str);
            mp_obj_t tuple[3];
            tuple[0] = mp_obj_new_str(ip_str, (size_t)ip_len);
            tuple[1] = mp_obj_new_int(sp);
            tuple[2] = mp_obj_new_str((const char *)&rbuf[6], dlen);
            return mp_obj_new_tuple(3, tuple);
        }
        extern void vios_net_yield(void);
        vios_net_yield();
    }
    return mp_const_none;
}
MP_DEFINE_CONST_FUN_OBJ_1(vnet_udp_recv_obj, vnet_udp_recv);

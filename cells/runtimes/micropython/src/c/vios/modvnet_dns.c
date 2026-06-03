/*
 * ViOS MicroPython vnet DNS resolution helper.
 *
 * Provides `vnet_do_dns_query(hostname, hlen, out_ip, 16)` — a UDP A-record
 * lookup to the QEMU SLIRP DNS server at 10.0.2.3:53.  Called by vnet_resolve
 * in modvnet.c after static table + IPv4-literal checks fail.
 *
 * Wire format for SOCKET_UDP / BIND / SENDTO / RECVFROM mirrors bindings_net.rs.
 */

#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include "py/obj.h"

extern void     vios_net_send(size_t endpoint, const uint8_t *buf, size_t len);
extern intptr_t vios_net_recv(size_t from_id, uint8_t *buf, size_t buf_len);
extern void     vios_net_yield(void);

#define NET_ENDPOINT 6u
#define SOCKET_UDP   0x11u
#define CLOSE_OP     0x15u
#define BIND_OP      0x16u
#define SENDTO_OP    0x21u
#define RECVFROM_OP  0x22u

static const uint8_t DNS_SERVER[4] = {10, 0, 2, 3};

/* ── Low-level IPC helpers ───────────────────────────────────────────────── */

static uint64_t rd64(const uint8_t *b) {
    return (uint64_t)b[0]        | ((uint64_t)b[1]<<8)  |
           ((uint64_t)b[2]<<16)  | ((uint64_t)b[3]<<24) |
           ((uint64_t)b[4]<<32)  | ((uint64_t)b[5]<<40) |
           ((uint64_t)b[6]<<48)  | ((uint64_t)b[7]<<56);
}
static void wr64(uint8_t *b, uint64_t v) {
    b[0]=(uint8_t)v;       b[1]=(uint8_t)(v>>8);
    b[2]=(uint8_t)(v>>16); b[3]=(uint8_t)(v>>24);
    b[4]=(uint8_t)(v>>32); b[5]=(uint8_t)(v>>40);
    b[6]=(uint8_t)(v>>48); b[7]=(uint8_t)(v>>56);
}
static void wr16(uint8_t *b, uint16_t v) {
    b[0]=(uint8_t)v; b[1]=(uint8_t)(v>>8);
}

/* ── DNS message builders / parsers ─────────────────────────────────────── */

/* Build a minimal DNS A-record query for hostname[0..hlen] into buf.
 * Returns number of bytes written. buf must be >= 17 + hlen bytes. */
static size_t build_dns_query(const char *hostname, size_t hlen, uint8_t *buf) {
    /* Fixed 12-byte header: ID=0x1234, QR=0 RD=1, QDCOUNT=1. */
    static const uint8_t HDR[12] = {
        0x12,0x34, 0x01,0x00, 0x00,0x01, 0,0, 0,0, 0,0
    };
    memcpy(buf, HDR, 12);
    size_t pos = 12;
    const char *p = hostname, *end = hostname + hlen;
    while (p < end) {
        const char *dot = p;
        while (dot < end && *dot != '.') dot++;
        size_t llen = (size_t)(dot - p);
        if (llen == 0) { p = dot + 1; continue; }
        buf[pos++] = (uint8_t)llen;
        memcpy(&buf[pos], p, llen);
        pos += llen;
        p = dot < end ? dot + 1 : end;
    }
    buf[pos++] = 0; /* root label */
    /* QTYPE=A (1), QCLASS=IN (1) */
    buf[pos++]=0; buf[pos++]=1; buf[pos++]=0; buf[pos++]=1;
    return pos;
}

/* Skip an encoded DNS name (label sequence or 2-byte compression pointer).
 * Returns position after the name, or 0 on error. */
static size_t skip_dns_name(const uint8_t *buf, size_t len, size_t pos) {
    while (pos < len) {
        uint8_t c = buf[pos];
        if (c == 0) return pos + 1;
        if ((c & 0xC0) == 0xC0) return pos + 2; /* compression pointer */
        pos += 1 + c;
    }
    return 0; /* truncated */
}

/* Parse the first A record from a raw DNS response.
 * Returns 1 and fills ip[4] on success, 0 if not found or malformed. */
static int parse_dns_a(const uint8_t *buf, size_t len, uint8_t ip[4]) {
    if (len < 12) return 0;
    if (!(buf[2] & 0x80)) return 0; /* QR bit = response */
    uint16_t ancount = (uint16_t)(((unsigned)buf[6]<<8) | buf[7]);
    if (ancount == 0) return 0;
    /* Skip question section: 12-byte header + QNAME + QTYPE + QCLASS */
    size_t pos = skip_dns_name(buf, len, 12);
    if (!pos || pos + 4 > len) return 0;
    pos += 4;
    /* Walk answer records */
    uint16_t i;
    for (i = 0; i < ancount; i++) {
        pos = skip_dns_name(buf, len, pos);
        if (!pos || pos + 10 > len) return 0;
        uint16_t rtype  = (uint16_t)(((unsigned)buf[pos]<<8)   | buf[pos+1]);
        uint16_t rdlen  = (uint16_t)(((unsigned)buf[pos+8]<<8) | buf[pos+9]);
        pos += 10;
        if (rtype == 1 && rdlen == 4 && pos + 4 <= len) {
            ip[0]=buf[pos]; ip[1]=buf[pos+1]; ip[2]=buf[pos+2]; ip[3]=buf[pos+3];
            return 1;
        }
        pos += rdlen;
    }
    return 0;
}

/* ── Public: vnet_do_dns_query ───────────────────────────────────────────── */

/*
 * Perform a UDP A-record query for hostname[0..hlen] against 10.0.2.3:53.
 * On success, writes the dotted-decimal IP (no NUL) to out and returns
 * the number of bytes written.  Returns 0 on failure.
 *
 * out must point to at least 16 bytes.
 */
int vnet_do_dns_query(const char *hostname, size_t hlen,
                      char *out, size_t out_len)
{
    if (hlen == 0 || hlen > 253 || out_len < 16) return 0;

    /* Open a UDP socket. */
    uint8_t smsg[9] = {SOCKET_UDP,0,0,0,0,0,0,0,0};
    vios_net_send(NET_ENDPOINT, smsg, 9);
    uint8_t cr[8]; memset(cr, 0, 8);
    if (vios_net_recv(0, cr, 8) < 0) return 0;
    uint64_t cap = rd64(cr);
    if (!cap) return 0;

    /* Bind to an ephemeral port (port=0). */
    uint8_t bmsg[11]; memset(bmsg, 0, 11);
    bmsg[0] = BIND_OP; wr64(&bmsg[1], cap); /* port stays 0 */
    vios_net_send(NET_ENDPOINT, bmsg, 11);
    uint8_t pr[2]; memset(pr, 0, 2);
    if (vios_net_recv(0, pr, 2) < 0 ||
        ((uint16_t)pr[0] | ((uint16_t)pr[1]<<8)) == 0xFFFFu) {
        /* close and give up */
        uint8_t cl[9]={CLOSE_OP}; wr64(&cl[1],cap);
        vios_net_send(NET_ENDPOINT,cl,9); uint8_t dummy[1]; vios_net_recv(0,dummy,1);
        return 0;
    }

    /* Build query and send it. */
    uint8_t query[300]; memset(query, 0, sizeof(query));
    size_t qlen = build_dns_query(hostname, hlen, query);

    /* SENDTO: [0x21][cap:8][dns_ip:4][53:2 LE][query:qlen]  min 15 bytes */
    uint8_t stbuf[9 + 6 + 300]; memset(stbuf, 0, sizeof(stbuf));
    stbuf[0] = SENDTO_OP; wr64(&stbuf[1], cap);
    stbuf[9]=DNS_SERVER[0]; stbuf[10]=DNS_SERVER[1];
    stbuf[11]=DNS_SERVER[2]; stbuf[12]=DNS_SERVER[3];
    wr16(&stbuf[13], 53u);
    memcpy(&stbuf[15], query, qlen);
    size_t stlen = 15 + qlen;
    vios_net_send(NET_ENDPOINT, stbuf, stlen);
    uint8_t cnt[4]; vios_net_recv(0, cnt, 4); /* drain TX ack */

    /* Poll for reply. */
    uint8_t rqbuf[13]; memset(rqbuf, 0, 13);
    rqbuf[0] = RECVFROM_OP; wr64(&rqbuf[1], cap);
    rqbuf[9]=0; rqbuf[10]=2; /* buf_len = 512 */
    static uint8_t reply[6+512];
    int found = 0; int tries;
    for (tries = 0; tries < 500 && !found; tries++) {
        memset(reply, 0, sizeof(reply));
        vios_net_send(NET_ENDPOINT, rqbuf, 13);
        if (vios_net_recv(0, reply, sizeof(reply)) >= 0 && reply[0] != 0) {
            uint8_t ip[4];
            if (parse_dns_a(&reply[6], sizeof(reply)-6, ip)) {
                /* Format IP */
                int n=0, oi;
                for (oi=0; oi<4; oi++) {
                    if (oi>0) out[n++]='.';
                    unsigned v=ip[oi];
                    if (v>=100){ out[n++]=(char)('0'+v/100); v%=100; out[n++]=(char)('0'+v/10); }
                    else if(v>=10) out[n++]=(char)('0'+v/10);
                    out[n++]=(char)('0'+v%10);
                }
                found = n;
            } else {
                found = -1; /* got a reply but no A record */
            }
        } else {
            vios_net_yield();
        }
    }

    /* Always close — RAII discipline. */
    uint8_t cl[9]={CLOSE_OP}; wr64(&cl[1],cap);
    vios_net_send(NET_ENDPOINT, cl, 9);
    uint8_t dummy2[1]; vios_net_recv(0, dummy2, 1);

    return found > 0 ? found : 0;
}

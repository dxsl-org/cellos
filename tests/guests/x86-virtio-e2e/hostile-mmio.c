#include <stdint.h>

#define MMIO_BASE 0xd0000000UL
#define NET_MMIO_OFFSET 0x200UL
#define PAGE_SIZE 4096UL
#define PROT_RW 3
#define MAP_SHARED 1
#define MAP_PRIVATE 2
#define MAP_ANON 0x20
#define O_RDWR 2
#define O_SYNC 04010000

struct desc { uint64_t addr; uint32_t len; uint16_t flags; uint16_t next; };
struct timespec { long sec; long nsec; };

static long call6(long n, long a, long b, long c, long d, long e, long f) {
    register long r10 __asm__("r10") = d;
    register long r8 __asm__("r8") = e;
    register long r9 __asm__("r9") = f;
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c),
                     "r"(r10), "r"(r8), "r"(r9) : "rcx", "r11", "memory");
    return r;
}
static long call3(long n, long a, long b, long c) { return call6(n, a, b, c, 0, 0, 0); }
static void emit(const char *s, long n) { (void)call3(1, 1, (long)s, n); }
static void uart_emit(const char *s, long n) {
    const uint16_t port = 0x3f8;
    for (long i = 0; i < n; i++)
        __asm__ volatile("outb %0, %1" : : "a"(s[i]), "Nd"(port) : "memory");
}
#define UART_EMIT(s) uart_emit(s, sizeof(s) - 1)
#define EMIT(s) emit(s, sizeof(s) - 1)
static void pause_after(void) {
    struct timespec t = { 2, 0 };
    (void)call3(35, (long)&t, 0, 0);
}
static void sleep_ms(long ms) {
    struct timespec t = { ms / 1000, (ms % 1000) * 1000000 };
    (void)call3(35, (long)&t, 0, 0);
}
static void fail(const char *s, long n) {
    EMIT("VIRTIO_HOSTILE_FAIL:"); emit(s, n); EMIT("\n"); call3(60, 1, 0, 0);
    __builtin_unreachable();
}
#define FAIL(s) fail(s, sizeof(s) - 1)
static void blocked(const char *s, long n) {
    EMIT("VIRTIO_HOSTILE_BLOCKED:"); emit(s, n); EMIT("\n");
    for (;;) pause_after();
}
#define BLOCKED(s) blocked(s, sizeof(s) - 1)
static void exercise_vcpu_preemption(void) {
    const uint16_t port = 0x5050;
    const uint32_t arm = 0x50524545;
    struct timespec started = { 0, 0 };
    struct timespec now = { 0, 0 };
    volatile uint64_t state = 1;
    if (call3(228, 1, (long)&started, 0) < 0) FAIL("clock-gettime-start");
    __asm__ volatile("outl %0, %1" : : "a"(arm), "Nd"(port) : "memory");
    do {
        for (unsigned i = 0; i < 100000; i++)
            state = state * 6364136223846793005UL + 1;
        if (call3(228, 1, (long)&now, 0) < 0) FAIL("clock-gettime-loop");
    } while ((now.sec - started.sec) * 1000000000L
             + now.nsec - started.nsec < 100000000L);
    if (state == 0) FAIL("preemption-spin-state");
}


static volatile uint32_t *mmio;
static volatile uint32_t *blk_mmio;
static volatile uint32_t *net_mmio;
static uint8_t *ring;
static uint64_t ring_gpa;
static uint64_t avail_gpa;
static uint64_t used_gpa;

static void wr(uint32_t off, uint32_t value) { mmio[off / 4] = value; __sync_synchronize(); }
static void reset(void) { wr(0x70, 0); }
static void addr(uint32_t lo, uint64_t value) {
    wr(lo, (uint32_t)value); wr(lo + 4, (uint32_t)(value >> 32));
}
static void handshake(void) {
    wr(0x70, 1); wr(0x70, 3); wr(0x24, 1); wr(0x20, 1); wr(0x70, 11);
}
static void queue_at(uint32_t index, uint32_t num, uint64_t desc, uint64_t avail,
                     uint64_t used) {
    wr(0x30, index); wr(0x38, num); addr(0x80, desc); addr(0x90, avail);
    addr(0xa0, used); wr(0x44, 1);
}
static void queue(uint32_t num, uint64_t desc, uint64_t avail, uint64_t used) {
    queue_at(0, num, desc, avail, used);
}
static void valid_queue(void) { handshake(); queue(8, ring_gpa, avail_gpa, used_gpa); }
#define SCENARIO(name, ...) do { reset(); UART_EMIT("[VIRTIO_HOSTILE] START " name "\n"); \
    __VA_ARGS__; UART_EMIT("[VIRTIO_HOSTILE] DONE " name "\n"); } while (0)

static uint64_t physical_page(void *p) {
    long fd = call3(2, (long)"/proc/self/pagemap", 0, 0);
    if (fd < 0) BLOCKED("guest kernel must expose /proc/self/pagemap to root");
    uint64_t entry = 0;
    uint64_t offset = ((uint64_t)p / PAGE_SIZE) * 8;
    long got = call6(17, fd, (long)&entry, 8, offset, 0, 0);
    (void)call3(3, fd, 0, 0);
    if (got != 8 || !(entry >> 63) || !(entry & ((1UL << 55) - 1)))
        BLOCKED("guest kernel must expose pagemap PFNs to a root init helper");
    return (entry & ((1UL << 55) - 1)) * PAGE_SIZE;
}
static void clear_ring(void) {
    for (unsigned i = 0; i < PAGE_SIZE * 3; i++) ring[i] = 0;
}
static struct desc *descs(void) { return (struct desc *)ring; }
static volatile uint16_t *avail_idx(void) { return (uint16_t *)(ring + PAGE_SIZE + 2); }
static volatile uint16_t *avail_slot(unsigned i) { return (uint16_t *)(ring + PAGE_SIZE + 4 + 2 * i); }
static volatile uint16_t *used_idx(void) { return (uint16_t *)(ring + 2 * PAGE_SIZE + 2); }
static void notify_queue(uint32_t index) { wr(0x50, index); }
static void notify(void) { notify_queue(0); }

static int try_recovery_write(void) {
    static const char marker[] = "CELLOS_X86_VIRTIO_HOSTILE_RECOVERY_V1";
    clear_ring(); reset(); valid_queue(); wr(0x70, 15);
    uint8_t *hdr = ring + 256, *data = ring + 512, *status = ring + 768;
    for (unsigned i = 0; i < 16; i++) hdr[i] = 0;
    hdr[0] = 1;
    for (unsigned i = 0; i < sizeof(marker) - 1; i++) data[i] = marker[i];
    *status = 0xff;
    descs()[0] = (struct desc){ ring_gpa + 256, 16, 1, 1 };
    descs()[1] = (struct desc){ ring_gpa + 512, sizeof(marker) - 1, 1, 2 };
    descs()[2] = (struct desc){ ring_gpa + 768, 1, 2, 0 };
    *avail_slot(0) = 0; *avail_idx() = 1; notify();
    if (*used_idx() != 1 || *status != 0) return 0;
    for (unsigned i = 0; i < 16; i++) hdr[i] = 0;
    hdr[0] = 4; *status = 0xff;
    descs()[3] = (struct desc){ ring_gpa + 256, 16, 1, 4 };
    descs()[4] = (struct desc){ ring_gpa + 768, 1, 2, 0 };
    *avail_slot(1) = 3; *avail_idx() = 2; notify();
    return *used_idx() == 2 && *status == 0;
}
static void recovery_write(void) {
    if (!try_recovery_write()) FAIL("recovery-write-flush-incomplete");
}
static int try_recovery_read(void) {
    static const char marker[] = "CELLOS_X86_VIRTIO_HOSTILE_RECOVERY_V1";
    clear_ring(); reset(); valid_queue(); wr(0x70, 15);
    uint8_t *hdr = ring + 256, *data = ring + 512, *status = ring + 768;
    for (unsigned i = 0; i < 16; i++) hdr[i] = 0;
    for (unsigned i = 0; i < sizeof(marker) - 1; i++) data[i] = 0;
    *status = 0xff;
    descs()[0] = (struct desc){ ring_gpa + 256, 16, 1, 1 };
    descs()[1] = (struct desc){ ring_gpa + 512, sizeof(marker) - 1, 3, 2 };
    descs()[2] = (struct desc){ ring_gpa + 768, 1, 2, 0 };
    *avail_slot(0) = 0; *avail_idx() = 1; notify();
    if (*used_idx() != 1 || *status != 0) return 0;
    for (unsigned i = 0; i < sizeof(marker) - 1; i++)
        if (data[i] != (uint8_t)marker[i]) return 0;
    return 1;
}
static void wait_recovery_read(void) {
    for (unsigned attempt = 0; attempt < 50; attempt++) {
        if (try_recovery_read()) return;
        sleep_ms(100);
    }
    FAIL("block-backend-did-not-recover-persisted-data");
}
static void block_backend_disconnect(void) {
    wr(0xfc, 1);
    clear_ring(); valid_queue(); wr(0x70, 15);
    uint8_t *hdr = ring + 256, *status = ring + 768;
    for (unsigned i = 0; i < 16; i++) hdr[i] = 0;
    hdr[0] = 4; *status = 0xff;
    descs()[0] = (struct desc){ ring_gpa + 256, 16, 1, 1 };
    descs()[1] = (struct desc){ ring_gpa + 768, 1, 2, 0 };
    *avail_slot(0) = 0; *avail_idx() = 1; notify();
    if (*used_idx() != 1 || *status != 1)
        FAIL("block-disconnect-did-not-return-ioerr");
}
static void unsupported_request(void) {
    clear_ring(); valid_queue(); wr(0x70, 15);
    uint8_t *hdr = ring + 256, *status = ring + 768;
    for (unsigned i = 0; i < 16; i++) hdr[i] = 0;
    hdr[0] = 0xff; *status = 0xff;
    descs()[0] = (struct desc){ ring_gpa + 256, 16, 1, 1 };
    descs()[1] = (struct desc){ ring_gpa + 768, 1, 2, 0 };
    *avail_slot(0) = 0; *avail_idx() = 1; notify();
    if (*used_idx() != 1 || *status != 2) FAIL("unsupported-request-not-rejected");
}
static void fill_arp_request(uint8_t *packet, uint8_t source_host) {
    for (unsigned i = 0; i < 12 + 42; i++) packet[i] = 0;
    for (unsigned i = 0; i < 6; i++) packet[12 + i] = 0xff;
    packet[18] = 0x52; packet[19] = 0x54; packet[20] = 0x00;
    packet[21] = 0xaa; packet[22] = 0xbb; packet[23] = 0xcc;
    packet[24] = 0x08; packet[25] = 0x06;
    packet[26] = 0x00; packet[27] = 0x01;
    packet[28] = 0x08; packet[29] = 0x00;
    packet[30] = 6; packet[31] = 4;
    packet[32] = 0x00; packet[33] = 0x01;
    for (unsigned i = 0; i < 6; i++) packet[34 + i] = packet[18 + i];
    packet[40] = 10; packet[41] = 0; packet[42] = 2; packet[43] = source_host;
    packet[50] = 10; packet[51] = 0; packet[52] = 2; packet[53] = 2;
}
static void net_tx_recovery(void) {
    clear_ring(); handshake();
    queue_at(1, 8, ring_gpa, avail_gpa, used_gpa); wr(0x70, 15);
    uint8_t *packet = ring + 256;
    fill_arp_request(packet, 15);
    descs()[0] = (struct desc){ ring_gpa + 256, 12 + 42, 0, 0 };
    *avail_slot(0) = 0; *avail_idx() = 1; notify_queue(1);
    if (*used_idx() != 1) FAIL("net-tx-recovery-incomplete");
}
static void net_backend_disconnect(void) {
    wr(0xfc, 1);
    net_tx_recovery();
}
static int try_net_backend_round_trip(uint8_t source_host) {
    clear_ring(); reset(); handshake();
    struct desc *rx_desc = (struct desc *)ring;
    struct desc *tx_desc = (struct desc *)(ring + 1024);
    volatile uint16_t *rx_avail_idx = (uint16_t *)(ring + PAGE_SIZE + 2);
    volatile uint16_t *rx_avail_slot = (uint16_t *)(ring + PAGE_SIZE + 4);
    volatile uint16_t *rx_used_idx = (uint16_t *)(ring + 2 * PAGE_SIZE + 2);
    volatile uint16_t *tx_avail_idx = (uint16_t *)(ring + PAGE_SIZE + 512 + 2);
    volatile uint16_t *tx_avail_slot = (uint16_t *)(ring + PAGE_SIZE + 512 + 4);
    volatile uint16_t *tx_used_idx = (uint16_t *)(ring + 2 * PAGE_SIZE + 512 + 2);
    uint8_t *rx_packet = ring + 2048;
    uint8_t *tx_packet = ring + 1280;
    rx_desc[0] = (struct desc){ ring_gpa + 2048, 512, 2, 0 };
    *rx_avail_slot = 0; *rx_avail_idx = 1;
    fill_arp_request(tx_packet, source_host);
    tx_desc[0] = (struct desc){ ring_gpa + 1280, 12 + 42, 0, 0 };
    *tx_avail_slot = 0; *tx_avail_idx = 1;
    queue_at(0, 8, ring_gpa, avail_gpa, used_gpa);
    queue_at(1, 8, ring_gpa + 1024, avail_gpa + 512, used_gpa + 512);
    wr(0x70, 15); notify_queue(1);
    if (*tx_used_idx != 1) return 0;
    notify_queue(0);
    for (unsigned attempt = 0; attempt < 20; attempt++) {
        if (*rx_used_idx == 1) {
            for (unsigned i = 0; i < 6; i++) {
                if (rx_packet[12 + i] != tx_packet[18 + i]) return 0;
                if (rx_packet[44 + i] != tx_packet[18 + i]) return 0;
            }
            return rx_packet[24] == 0x08 && rx_packet[25] == 0x06
                && rx_packet[32] == 0x00 && rx_packet[33] == 0x02
                && rx_packet[40] == 10 && rx_packet[41] == 0
                && rx_packet[42] == 2 && rx_packet[43] == 2
                && rx_packet[50] == 10 && rx_packet[51] == 0
                && rx_packet[52] == 2 && rx_packet[53] == source_host;
        }
        sleep_ms(100);
    }
    return 0;
}
static void wait_net_backend_recovery(void) {
    for (unsigned attempt = 0; attempt < 10; attempt++)
        if (try_net_backend_round_trip((uint8_t)(16 + attempt))) return;
    FAIL("net-backend-did-not-recover-tx-rx");
}


static int run(void) {
    long mem = call3(2, (long)"/dev/mem", O_RDWR | O_SYNC, 0);
    if (mem < 0) BLOCKED("guest kernel must permit root access to /dev/mem");
    long map = call6(9, 0, PAGE_SIZE, PROT_RW, MAP_SHARED, mem, MMIO_BASE);
    if (map < 0) BLOCKED("guest kernel must permit root mapping of the VirtIO MMIO page");
    if (call3(172, 3, 0, 0) < 0)
        BLOCKED("guest kernel must permit root iopl for synchronous UART delimiters");
    blk_mmio = (volatile uint32_t *)map;
    net_mmio = blk_mmio + NET_MMIO_OFFSET / sizeof(*blk_mmio);
    mmio = blk_mmio;
    if (mmio[0] != 0x74726976 || mmio[2] != 2)
        BLOCKED("nested x86 guest must expose the Phase-10 VirtIO block MMIO slot");
    if (net_mmio[0] != 0x74726976 || net_mmio[2] != 1)
        BLOCKED("nested x86 guest must expose the Phase-10 VirtIO net MMIO slot");
    long pages = call6(9, 0, PAGE_SIZE * 3, PROT_RW, MAP_PRIVATE | MAP_ANON, -1, 0);
    if (pages < 0) FAIL("ring-allocation");
    ring = (uint8_t *)pages; clear_ring(); (void)call3(149, pages, PAGE_SIZE * 3, 0);
    ring_gpa = physical_page(ring);
    avail_gpa = physical_page(ring + PAGE_SIZE);
    used_gpa = physical_page(ring + 2 * PAGE_SIZE);

    SCENARIO("vcpu-preemption", exercise_vcpu_preemption());
    SCENARIO("invalid-queue-select", wr(0x30, 0); wr(0x38, 8);
        wr(0x30, 0xffffffff); wr(0x38, 3); wr(0x30, 0);
        if (mmio[0x38 / 4] != 8) FAIL("invalid-queue-select-left-stale-queue"));
    SCENARIO("queue-size-zero", handshake(); queue(0, ring_gpa, avail_gpa, used_gpa));
    SCENARIO("queue-size-non-power-two", handshake(); queue(3, ring_gpa, avail_gpa, used_gpa));
    SCENARIO("queue-size-oversize", handshake(); queue(257, ring_gpa, avail_gpa, used_gpa));
    SCENARIO("descriptor-zero", handshake(); queue(8, 0, avail_gpa, used_gpa));
    SCENARIO("descriptor-misaligned", handshake(); queue(8, ring_gpa + 8, avail_gpa, used_gpa));
    SCENARIO("avail-zero", handshake(); queue(8, ring_gpa, 0, used_gpa));
    SCENARIO("avail-misaligned", handshake(); queue(8, ring_gpa, avail_gpa + 1, used_gpa));
    SCENARIO("used-zero", handshake(); queue(8, ring_gpa, avail_gpa, 0));
    SCENARIO("used-misaligned", handshake(); queue(8, ring_gpa, avail_gpa, used_gpa + 2));
    SCENARIO("descriptor-span-overflow", handshake(); queue(256, 0xfffffffffffffff0UL, avail_gpa, used_gpa));
    SCENARIO("avail-span-overflow", handshake(); queue(256, ring_gpa, 0xfffffffffffffffeUL, used_gpa));
    SCENARIO("used-span-overflow", handshake(); queue(256, ring_gpa, avail_gpa, 0xfffffffffffffffcUL));
    SCENARIO("notify-before-driver-ok", handshake(); queue(8, ring_gpa, avail_gpa, used_gpa); notify());
    SCENARIO("notify-invalid-config", valid_queue(); addr(0x90, 0); wr(0x70, 15); notify());
    SCENARIO("reset-clears-state", valid_queue(); wr(0x70, 15); reset();
        wr(0x30, 0);
        if (mmio[0x38 / 4] != 0 || mmio[0x70 / 4] != 0)
            FAIL("reset-left-transport-state"));

    SCENARIO("pending-index-delta", clear_ring(); valid_queue(); wr(0x70, 15); *avail_idx() = 9; notify());
    SCENARIO("descriptor-head-oob", clear_ring(); valid_queue(); wr(0x70, 15); *avail_slot(0) = 8; *avail_idx() = 1; notify());
    SCENARIO("descriptor-next-oob", clear_ring(); valid_queue(); wr(0x70, 15); descs()[0] = (struct desc){ring_gpa + 256, 16, 1, 8}; *avail_slot(0) = 0; *avail_idx() = 1; notify());
    SCENARIO("descriptor-payload-overflow", clear_ring(); valid_queue(); wr(0x70, 15); descs()[0] = (struct desc){0xfffffffffffffff8UL, 16, 0, 0}; *avail_slot(0) = 0; *avail_idx() = 1; notify());
    SCENARIO("backend-unsupported-opcode", unsupported_request());
    recovery_write();
    SCENARIO("backend-disconnect", block_backend_disconnect());
    SCENARIO("backend-reconnect", wait_recovery_read());
    mmio = net_mmio;
    SCENARIO("net-recovery-sentinel", net_tx_recovery());
    SCENARIO("net-backend-disconnect", net_backend_disconnect());
    SCENARIO("net-backend-reconnect", wait_net_backend_recovery());
    mmio = blk_mmio;


    UART_EMIT("[VIRTIO_HOSTILE] START recovery-write-flush\n"); recovery_write();
    UART_EMIT("[VIRTIO_HOSTILE] DONE recovery-write-flush\n");
    for (;;) pause_after();
}

void _start(void) { call3(60, run(), 0, 0); __builtin_unreachable(); }

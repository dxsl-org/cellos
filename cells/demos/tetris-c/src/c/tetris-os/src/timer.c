#include "timer.h"

#include "io.h"
#include "pic.h"

#define PIT_COMMAND 0x43
#define PIT_CHANNEL0 0x40
#define PIT_FREQUENCY 1193180U
#define TIMER_HZ 100U

static volatile uint32_t ticks;

void timer_init(void)
{
    uint16_t divisor = (uint16_t)(PIT_FREQUENCY / TIMER_HZ);
    ticks = 0;
    outb(PIT_COMMAND, 0x36);
    outb(PIT_CHANNEL0, (uint8_t)(divisor & 0xFFU));
    outb(PIT_CHANNEL0, (uint8_t)((divisor >> 8) & 0xFFU));
}

uint32_t timer_get_ticks(void)
{
    return ticks;
}

void irq0_handler(void)
{
    ticks++;
    pic_send_eoi(0);
}

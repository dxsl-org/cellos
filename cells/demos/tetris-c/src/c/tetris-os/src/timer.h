#ifndef TETRIS_OS_TIMER_H
#define TETRIS_OS_TIMER_H

#include <stdint.h>

void timer_init(void);
uint32_t timer_get_ticks(void);
void irq0_handler(void);

#endif

#ifndef TETRIS_OS_IDT_H
#define TETRIS_OS_IDT_H

#include <stdint.h>

void idt_init(void);
void isr_exception_handler(uint32_t vector);

#endif

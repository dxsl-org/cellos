#include "idt.h"

#include "io.h"
#include "pic.h"
#include "vga.h"

struct idt_entry {
    uint16_t offset_low;
    uint16_t selector;
    uint8_t zero;
    uint8_t type_attr;
    uint16_t offset_high;
} __attribute__((packed));

struct idt_ptr {
    uint16_t limit;
    uint32_t base;
} __attribute__((packed));

#define IDT_ENTRIES 256
#define KERNEL_CS 0x08
#define IDT_INTERRUPT_GATE 0x8E

static volatile struct idt_entry idt[IDT_ENTRIES];

extern void isr0(void);
extern void isr1(void);
extern void isr2(void);
extern void isr3(void);
extern void isr4(void);
extern void isr5(void);
extern void isr6(void);
extern void isr7(void);
extern void isr8(void);
extern void isr9(void);
extern void isr10(void);
extern void isr11(void);
extern void isr12(void);
extern void isr13(void);
extern void isr14(void);
extern void isr15(void);
extern void isr16(void);
extern void isr17(void);
extern void isr18(void);
extern void isr19(void);
extern void isr20(void);
extern void isr21(void);
extern void isr22(void);
extern void isr23(void);
extern void isr24(void);
extern void isr25(void);
extern void isr26(void);
extern void isr27(void);
extern void isr28(void);
extern void isr29(void);
extern void isr30(void);
extern void isr31(void);
extern void irq0(void);
extern void irq1(void);

static void idt_set_gate(uint8_t vector, void (*handler)(void))
{
    uint32_t addr = (uint32_t)handler;
    idt[vector].offset_low = (uint16_t)(addr & 0xFFFFU);
    idt[vector].selector = KERNEL_CS;
    idt[vector].zero = 0;
    idt[vector].type_attr = IDT_INTERRUPT_GATE;
    idt[vector].offset_high = (uint16_t)((addr >> 16) & 0xFFFFU);
}

static void idt_load(const struct idt_ptr *ptr)
{
    __asm__ volatile ("lidt %0" : : "m"(*ptr) : "memory");
}

void idt_init(void)
{
    struct idt_ptr ptr;

    for (uint32_t i = 0; i < IDT_ENTRIES; i++) {
        idt[i].offset_low = 0;
        idt[i].selector = 0;
        idt[i].zero = 0;
        idt[i].type_attr = 0;
        idt[i].offset_high = 0;
    }

    void (*exceptions[32])(void) = {
        isr0, isr1, isr2, isr3, isr4, isr5, isr6, isr7,
        isr8, isr9, isr10, isr11, isr12, isr13, isr14, isr15,
        isr16, isr17, isr18, isr19, isr20, isr21, isr22, isr23,
        isr24, isr25, isr26, isr27, isr28, isr29, isr30, isr31
    };

    for (uint8_t i = 0; i < 32; i++) {
        idt_set_gate(i, exceptions[i]);
    }

    idt_set_gate(0x20, irq0);
    idt_set_gate(0x21, irq1);

    ptr.limit = (uint16_t)(sizeof(idt) - 1U);
    ptr.base = (uint32_t)idt;
    idt_load(&ptr);
}

void isr_exception_handler(uint32_t vector)
{
    interrupts_disable();
    vga_clear(0);
    vga_draw_string(72, 88, "CPU EXCEPTION", 4, 0);
    vga_draw_string(104, 104, "VECTOR", 7, 0);
    vga_draw_number(160, 104, vector, 7, 0);

    for (;;) {
        __asm__ volatile ("hlt");
    }
}

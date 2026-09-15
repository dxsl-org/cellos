#include <stddef.h>
#include <stdint.h>

#include "idt.h"
#include "io.h"
#include "keyboard.h"
#include "pic.h"
#include "speaker.h"
#include "timer.h"
#include "tetris.h"
#include "vga.h"

void *memset(void *dest, int value, size_t count)
{
    unsigned char *out = (unsigned char *)dest;
    while (count-- != 0U) {
        *out++ = (unsigned char)value;
    }
    return dest;
}

void *memcpy(void *dest, const void *src, size_t count)
{
    unsigned char *out = (unsigned char *)dest;
    const unsigned char *in = (const unsigned char *)src;
    while (count-- != 0U) {
        *out++ = *in++;
    }
    return dest;
}

size_t strlen(const char *text)
{
    size_t len = 0;
    while (text[len] != '\0') {
        len++;
    }
    return len;
}

void kernel_main(uint32_t multiboot_magic, uint32_t multiboot_info)
{
    (void)multiboot_magic;
    (void)multiboot_info;

    interrupts_disable();
    vga_init();
    idt_init();
    pic_init();
    timer_init();
    keyboard_init();
    speaker_init();
    interrupts_enable();

    tetris_run();

    for (;;) {
        __asm__ volatile ("hlt");
    }
}

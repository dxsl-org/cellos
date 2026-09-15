#ifndef TETRIS_OS_VGA_H
#define TETRIS_OS_VGA_H

#include <stdint.h>

void vga_init(void);
void vga_put_pixel(int x, int y, uint8_t color);
void vga_fill_rect(int x, int y, int w, int h, uint8_t color);
void vga_draw_char(int x, int y, char c, uint8_t fg, uint8_t bg);
void vga_draw_string(int x, int y, const char *text, uint8_t fg, uint8_t bg);
void vga_draw_number(int x, int y, uint32_t value, uint8_t fg, uint8_t bg);
void vga_clear(uint8_t color);
void vga_present(void);

#endif

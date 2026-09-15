#include "keyboard.h"

#include <stdbool.h>
#include <stdint.h>

#include "io.h"
#include "pic.h"

#define KEYBOARD_DATA 0x60
#define KEYBOARD_STATUS 0x64

static volatile int last_key;
static bool left_shift;
static bool right_shift;
static bool caps_lock;
static bool extended;
static enum keyboard_layout active_layout;

static const int german_normal_map[128] = {
    [0x01] = KEY_ESCAPE,
    [0x02] = '1',
    [0x03] = '2',
    [0x04] = '3',
    [0x05] = '4',
    [0x06] = '5',
    [0x07] = '6',
    [0x08] = '7',
    [0x09] = '8',
    [0x0A] = '9',
    [0x0B] = '0',
    [0x0C] = KEY_SHARP_S,
    [0x0D] = '\'',
    [0x0E] = KEY_BACKSPACE,
    [0x0F] = '\t',
    [0x10] = 'q',
    [0x11] = 'w',
    [0x12] = 'e',
    [0x13] = 'r',
    [0x14] = 't',
    [0x15] = 'z',
    [0x16] = 'u',
    [0x17] = 'i',
    [0x18] = 'o',
    [0x19] = 'p',
    [0x1A] = KEY_UMLAUT_U,
    [0x1B] = '+',
    [0x1C] = KEY_ENTER,
    [0x1E] = 'a',
    [0x1F] = 's',
    [0x20] = 'd',
    [0x21] = 'f',
    [0x22] = 'g',
    [0x23] = 'h',
    [0x24] = 'j',
    [0x25] = 'k',
    [0x26] = 'l',
    [0x27] = KEY_UMLAUT_O,
    [0x28] = KEY_UMLAUT_A,
    [0x29] = '^',
    [0x2B] = '#',
    [0x2C] = 'y',
    [0x2D] = 'x',
    [0x2E] = 'c',
    [0x2F] = 'v',
    [0x30] = 'b',
    [0x31] = 'n',
    [0x32] = 'm',
    [0x33] = ',',
    [0x34] = '.',
    [0x35] = '-',
    [0x39] = ' ',
    [0x47] = '7',
    [0x48] = '8',
    [0x49] = '9',
    [0x4A] = '-',
    [0x4B] = '4',
    [0x4C] = '5',
    [0x4D] = '6',
    [0x4E] = '+',
    [0x4F] = '1',
    [0x50] = '2',
    [0x51] = '3',
    [0x52] = '0',
    [0x53] = ','
};

static const int german_shift_map[128] = {
    [0x01] = KEY_ESCAPE,
    [0x02] = '!',
    [0x03] = '"',
    [0x04] = 0x15,
    [0x05] = '$',
    [0x06] = '%',
    [0x07] = '&',
    [0x08] = '/',
    [0x09] = '(',
    [0x0A] = ')',
    [0x0B] = '=',
    [0x0C] = '?',
    [0x0D] = '`',
    [0x0E] = KEY_BACKSPACE,
    [0x0F] = '\t',
    [0x10] = 'Q',
    [0x11] = 'W',
    [0x12] = 'E',
    [0x13] = 'R',
    [0x14] = 'T',
    [0x15] = 'Z',
    [0x16] = 'U',
    [0x17] = 'I',
    [0x18] = 'O',
    [0x19] = 'P',
    [0x1A] = KEY_UMLAUT_U,
    [0x1B] = '*',
    [0x1C] = KEY_ENTER,
    [0x1E] = 'A',
    [0x1F] = 'S',
    [0x20] = 'D',
    [0x21] = 'F',
    [0x22] = 'G',
    [0x23] = 'H',
    [0x24] = 'J',
    [0x25] = 'K',
    [0x26] = 'L',
    [0x27] = KEY_UMLAUT_O,
    [0x28] = KEY_UMLAUT_A,
    [0x29] = 0xF8,
    [0x2B] = '\'',
    [0x2C] = 'Y',
    [0x2D] = 'X',
    [0x2E] = 'C',
    [0x2F] = 'V',
    [0x30] = 'B',
    [0x31] = 'N',
    [0x32] = 'M',
    [0x33] = ';',
    [0x34] = ':',
    [0x35] = '_',
    [0x39] = ' ',
    [0x47] = '7',
    [0x48] = '8',
    [0x49] = '9',
    [0x4A] = '-',
    [0x4B] = '4',
    [0x4C] = '5',
    [0x4D] = '6',
    [0x4E] = '+',
    [0x4F] = '1',
    [0x50] = '2',
    [0x51] = '3',
    [0x52] = '0',
    [0x53] = ','
};

static const int us_normal_map[128] = {
    [0x01] = KEY_ESCAPE,
    [0x02] = '1',
    [0x03] = '2',
    [0x04] = '3',
    [0x05] = '4',
    [0x06] = '5',
    [0x07] = '6',
    [0x08] = '7',
    [0x09] = '8',
    [0x0A] = '9',
    [0x0B] = '0',
    [0x0C] = '-',
    [0x0D] = '=',
    [0x0E] = KEY_BACKSPACE,
    [0x0F] = '\t',
    [0x10] = 'q',
    [0x11] = 'w',
    [0x12] = 'e',
    [0x13] = 'r',
    [0x14] = 't',
    [0x15] = 'y',
    [0x16] = 'u',
    [0x17] = 'i',
    [0x18] = 'o',
    [0x19] = 'p',
    [0x1A] = '[',
    [0x1B] = ']',
    [0x1C] = KEY_ENTER,
    [0x1E] = 'a',
    [0x1F] = 's',
    [0x20] = 'd',
    [0x21] = 'f',
    [0x22] = 'g',
    [0x23] = 'h',
    [0x24] = 'j',
    [0x25] = 'k',
    [0x26] = 'l',
    [0x27] = ';',
    [0x28] = '\'',
    [0x29] = '`',
    [0x2B] = '\\',
    [0x2C] = 'z',
    [0x2D] = 'x',
    [0x2E] = 'c',
    [0x2F] = 'v',
    [0x30] = 'b',
    [0x31] = 'n',
    [0x32] = 'm',
    [0x33] = ',',
    [0x34] = '.',
    [0x35] = '/',
    [0x39] = ' ',
    [0x47] = '7',
    [0x48] = '8',
    [0x49] = '9',
    [0x4A] = '-',
    [0x4B] = '4',
    [0x4C] = '5',
    [0x4D] = '6',
    [0x4E] = '+',
    [0x4F] = '1',
    [0x50] = '2',
    [0x51] = '3',
    [0x52] = '0',
    [0x53] = '.'
};

static const int us_shift_map[128] = {
    [0x01] = KEY_ESCAPE,
    [0x02] = '!',
    [0x03] = '@',
    [0x04] = '#',
    [0x05] = '$',
    [0x06] = '%',
    [0x07] = '^',
    [0x08] = '&',
    [0x09] = '*',
    [0x0A] = '(',
    [0x0B] = ')',
    [0x0C] = '_',
    [0x0D] = '+',
    [0x0E] = KEY_BACKSPACE,
    [0x0F] = '\t',
    [0x10] = 'Q',
    [0x11] = 'W',
    [0x12] = 'E',
    [0x13] = 'R',
    [0x14] = 'T',
    [0x15] = 'Y',
    [0x16] = 'U',
    [0x17] = 'I',
    [0x18] = 'O',
    [0x19] = 'P',
    [0x1A] = '{',
    [0x1B] = '}',
    [0x1C] = KEY_ENTER,
    [0x1E] = 'A',
    [0x1F] = 'S',
    [0x20] = 'D',
    [0x21] = 'F',
    [0x22] = 'G',
    [0x23] = 'H',
    [0x24] = 'J',
    [0x25] = 'K',
    [0x26] = 'L',
    [0x27] = ':',
    [0x28] = '"',
    [0x29] = '~',
    [0x2B] = '|',
    [0x2C] = 'Z',
    [0x2D] = 'X',
    [0x2E] = 'C',
    [0x2F] = 'V',
    [0x30] = 'B',
    [0x31] = 'N',
    [0x32] = 'M',
    [0x33] = '<',
    [0x34] = '>',
    [0x35] = '?',
    [0x39] = ' ',
    [0x47] = '7',
    [0x48] = '8',
    [0x49] = '9',
    [0x4A] = '-',
    [0x4B] = '4',
    [0x4C] = '5',
    [0x4D] = '6',
    [0x4E] = '+',
    [0x4F] = '1',
    [0x50] = '2',
    [0x51] = '3',
    [0x52] = '0',
    [0x53] = '.'
};

static bool is_letter(int key)
{
    return key >= 'a' && key <= 'z';
}

static int translate_key(uint8_t scancode)
{
    bool shift = left_shift || right_shift;
    const int *normal_map = active_layout == KEYBOARD_LAYOUT_US ? us_normal_map : german_normal_map;
    const int *shift_map = active_layout == KEYBOARD_LAYOUT_US ? us_shift_map : german_shift_map;
    int key = shift ? shift_map[scancode] : normal_map[scancode];

    if (key == 0) {
        return KEY_NONE;
    }

    if (is_letter(key) && caps_lock) {
        key = key - 'a' + 'A';
    } else if (key >= 'A' && key <= 'Z' && caps_lock) {
        key = key - 'A' + 'a';
    }

    return key;
}

void keyboard_init(void)
{
    last_key = KEY_NONE;
    left_shift = false;
    right_shift = false;
    caps_lock = false;
    extended = false;
    active_layout = KEYBOARD_LAYOUT_DE;

    while ((inb(KEYBOARD_STATUS) & 1U) != 0U) {
        (void)inb(KEYBOARD_DATA);
    }
}

void keyboard_set_layout(enum keyboard_layout layout)
{
    active_layout = layout;
    last_key = KEY_NONE;
}

int keyboard_get_key(void)
{
    int key = last_key;
    last_key = KEY_NONE;
    return key;
}

void irq1_handler(void)
{
    uint8_t scancode = inb(KEYBOARD_DATA);
    bool released = (scancode & 0x80U) != 0U;
    uint8_t code = scancode & 0x7FU;

    if (scancode == 0xE0U) {
        extended = true;
        pic_send_eoi(1);
        return;
    }

    if (code == 0x2AU) {
        left_shift = !released;
        extended = false;
        pic_send_eoi(1);
        return;
    }

    if (code == 0x36U) {
        right_shift = !released;
        extended = false;
        pic_send_eoi(1);
        return;
    }

    if (!released && code == 0x3AU) {
        caps_lock = !caps_lock;
        last_key = KEY_CAPSLOCK;
        extended = false;
        pic_send_eoi(1);
        return;
    }

    if (!released) {
        if (extended) {
            if (code == 0x48U) {
                last_key = KEY_UP;
            } else if (code == 0x50U) {
                last_key = KEY_DOWN;
            } else if (code == 0x4BU) {
                last_key = KEY_LEFT;
            } else if (code == 0x4DU) {
                last_key = KEY_RIGHT;
            }
        } else {
            last_key = translate_key(code);
        }
    }

    extended = false;
    pic_send_eoi(1);
}

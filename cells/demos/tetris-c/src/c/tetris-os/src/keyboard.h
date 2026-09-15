#ifndef TETRIS_OS_KEYBOARD_H
#define TETRIS_OS_KEYBOARD_H

enum {
    KEY_NONE = 0,
    KEY_ESCAPE = 256,
    KEY_BACKSPACE,
    KEY_ENTER,
    KEY_LEFT,
    KEY_RIGHT,
    KEY_UP,
    KEY_DOWN,
    KEY_CAPSLOCK,
    KEY_UMLAUT_A,
    KEY_UMLAUT_O,
    KEY_UMLAUT_U,
    KEY_SHARP_S
};

enum keyboard_layout {
    KEYBOARD_LAYOUT_DE,
    KEYBOARD_LAYOUT_US
};

void keyboard_init(void);
void keyboard_set_layout(enum keyboard_layout layout);
int keyboard_get_key(void);
void irq1_handler(void);

#endif

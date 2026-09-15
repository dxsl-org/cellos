#include "speaker.h"

#include <stdbool.h>
#include <stdint.h>

#include "io.h"
#include "timer.h"

#define PIT_COMMAND 0x43
#define PIT_CHANNEL2 0x42
#define PIT_FREQUENCY 1193180U
#define SPEAKER_PORT 0x61
#define MUSIC_TEMPO_SCALE 3U
#define MUSIC_GATE_NUMERATOR 45U
#define MUSIC_GATE_DENOMINATOR 100U

struct music_note {
    uint16_t hz;
    uint8_t ticks;
};

enum {
    REST = 0,
    A4 = 440,
    B4 = 494,
    C5 = 523,
    D5 = 587,
    E5 = 659,
    F5 = 698,
    G5 = 784,
    A5 = 880
};

/* Melody based on the public-domain Russian folk song "Korobeiniki".
 * This is a simple PC-speaker note sequence, not Nintendo's/Tetris's
 * copyrighted recording or arrangement.
 */
static const struct music_note melody[] = {
    {E5, 16}, {B4, 8}, {C5, 8}, {D5, 16}, {C5, 8}, {B4, 8},
    {A4, 16}, {A4, 8}, {C5, 8}, {E5, 16}, {D5, 8}, {C5, 8},
    {B4, 24}, {C5, 8}, {D5, 16}, {E5, 16}, {C5, 16}, {A4, 16},
    {A4, 24}, {REST, 8},

    {D5, 24}, {F5, 8}, {A5, 16}, {G5, 8}, {F5, 8},
    {E5, 24}, {C5, 8}, {E5, 16}, {D5, 8}, {C5, 8},
    {B4, 16}, {B4, 8}, {C5, 8}, {D5, 16}, {E5, 16},
    {C5, 16}, {A4, 16}, {A4, 24}, {REST, 8}
};

static bool music_playing;
static uint32_t next_note_tick;
static uint32_t note_off_tick;
static uint32_t note_index;
static bool note_is_on;

static void speaker_on(uint16_t hz)
{
    uint16_t divisor;
    uint8_t control;

    if (hz == 0U) {
        return;
    }

    divisor = (uint16_t)(PIT_FREQUENCY / hz);
    outb(PIT_COMMAND, 0xB6);
    outb(PIT_CHANNEL2, (uint8_t)(divisor & 0xFFU));
    outb(PIT_CHANNEL2, (uint8_t)((divisor >> 8) & 0xFFU));

    control = inb(SPEAKER_PORT);
    outb(SPEAKER_PORT, (uint8_t)(control | 0x03U));
}

static void speaker_off(void)
{
    outb(SPEAKER_PORT, (uint8_t)(inb(SPEAKER_PORT) & 0xFCU));
}

void speaker_init(void)
{
    music_playing = false;
    next_note_tick = 0;
    note_off_tick = 0;
    note_index = 0;
    note_is_on = false;
    speaker_off();
}

void speaker_music_start(void)
{
    music_playing = true;
    next_note_tick = 0;
    note_off_tick = 0;
    note_index = 0;
    note_is_on = false;
}

void speaker_music_stop(void)
{
    music_playing = false;
    note_is_on = false;
    speaker_off();
}

void speaker_music_update(void)
{
    const struct music_note *note;
    uint32_t now;
    uint32_t duration;
    uint32_t gated_duration;

    if (!music_playing) {
        return;
    }

    now = timer_get_ticks();
    if (note_is_on && now >= note_off_tick) {
        note_is_on = false;
        speaker_off();
    }

    if (now < next_note_tick) {
        return;
    }

    note = &melody[note_index];
    duration = (uint32_t)note->ticks * MUSIC_TEMPO_SCALE;
    gated_duration = (duration * MUSIC_GATE_NUMERATOR) / MUSIC_GATE_DENOMINATOR;
    if (gated_duration == 0U) {
        gated_duration = 1U;
    }

    if (note->hz == REST) {
        note_is_on = false;
        speaker_off();
    } else {
        speaker_on(note->hz);
        note_is_on = true;
    }

    note_off_tick = now + gated_duration;
    next_note_tick = now + duration;
    note_index++;
    if (note_index >= (sizeof(melody) / sizeof(melody[0]))) {
        note_index = 0;
    }
}

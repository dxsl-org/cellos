#include "tetris.h"

#include <stdbool.h>
#include <stdint.h>

#include "keyboard.h"
#include "speaker.h"
#include "timer.h"
#include "vga.h"

#define BOARD_W 10
#define BOARD_H 20
#define CELL 10
#define BOARD_X 110
#define BOARD_Y 0
#define PREVIEW_X 246
#define PREVIEW_Y 56
#define DROP_START_TICKS 80U
#define DROP_MIN_TICKS 12U
#define DROP_LEVEL_STEP 4U
#define DROP_TIME_STEP_TICKS 3000U

enum game_state {
    STATE_LAYOUT,
    STATE_TITLE,
    STATE_PLAYING,
    STATE_PAUSED,
    STATE_GAME_OVER
};

struct piece {
    int type;
    int rotation;
    int x;
    int y;
};

static uint8_t board[BOARD_H][BOARD_W];
static struct piece current_piece;
static int next_piece;
static enum game_state state;
static uint32_t score;
static uint32_t lines_cleared;
static uint32_t level;
static uint32_t last_drop_tick;
static uint32_t game_start_tick;
static uint32_t rng_state;

static const uint8_t piece_colors[7] = {9, 14, 5, 2, 4, 6, 1};

static const uint16_t shapes[7][4] = {
    {0x0F00, 0x2222, 0x00F0, 0x4444},
    {0x0660, 0x0660, 0x0660, 0x0660},
    {0x04E0, 0x4640, 0x0E40, 0x4C40},
    {0x06C0, 0x4620, 0x06C0, 0x4620},
    {0x0C60, 0x2640, 0x0C60, 0x2640},
    {0x02E0, 0x4460, 0x0E80, 0xC440},
    {0x08E0, 0x6440, 0x0E20, 0x44C0}
};

static const int kick_tests[5][2] = {
    {0, 0}, {-1, 0}, {1, 0}, {0, -1}, {-2, 0}
};

static uint32_t drop_interval(void)
{
    uint32_t elapsed_ticks = timer_get_ticks() - game_start_tick;
    uint32_t level_reduction = (level > 1U) ? ((level - 1U) * DROP_LEVEL_STEP) : 0U;
    uint32_t time_reduction = elapsed_ticks / DROP_TIME_STEP_TICKS;
    uint32_t reduction = level_reduction + time_reduction;
    uint32_t max_reduction = DROP_START_TICKS - DROP_MIN_TICKS;

    if (reduction > max_reduction) {
        reduction = max_reduction;
    }

    return DROP_START_TICKS - reduction;
}

static void clear_board(void)
{
    for (int y = 0; y < BOARD_H; y++) {
        for (int x = 0; x < BOARD_W; x++) {
            board[y][x] = 0;
        }
    }
}

static uint32_t rng_next(void)
{
    rng_state = (rng_state * 1103515245U) + 12345U;
    return (rng_state >> 16) & 0x7FFFU;
}

static int random_piece(void)
{
    return (int)(rng_next() % 7U);
}

static bool shape_cell(uint16_t shape, int x, int y)
{
    return (shape & (uint16_t)(0x8000U >> ((y * 4) + x))) != 0U;
}

static bool collides(const struct piece *piece, int test_x, int test_y, int test_rot)
{
    uint16_t shape = shapes[piece->type][test_rot & 3];

    for (int y = 0; y < 4; y++) {
        for (int x = 0; x < 4; x++) {
            if (!shape_cell(shape, x, y)) {
                continue;
            }

            int bx = test_x + x;
            int by = test_y + y;
            if (bx < 0 || bx >= BOARD_W || by >= BOARD_H) {
                return true;
            }
            if (by >= 0 && board[by][bx] != 0U) {
                return true;
            }
        }
    }

    return false;
}

static bool move_piece(int dx, int dy)
{
    int nx = current_piece.x + dx;
    int ny = current_piece.y + dy;
    if (collides(&current_piece, nx, ny, current_piece.rotation)) {
        return false;
    }

    current_piece.x = nx;
    current_piece.y = ny;
    return true;
}

static bool rotate_piece(void)
{
    int new_rot = (current_piece.rotation + 1) & 3;

    for (uint32_t i = 0; i < 5U; i++) {
        int nx = current_piece.x + kick_tests[i][0];
        int ny = current_piece.y + kick_tests[i][1];
        if (!collides(&current_piece, nx, ny, new_rot)) {
            current_piece.x = nx;
            current_piece.y = ny;
            current_piece.rotation = new_rot;
            return true;
        }
    }

    return false;
}

static uint32_t compact_lines(void)
{
    uint32_t cleared = 0;

    for (int y = BOARD_H - 1; y >= 0; y--) {
        bool full = true;
        for (int x = 0; x < BOARD_W; x++) {
            if (board[y][x] == 0U) {
                full = false;
                break;
            }
        }

        if (!full) {
            continue;
        }

        cleared++;
        for (int yy = y; yy > 0; yy--) {
            for (int x = 0; x < BOARD_W; x++) {
                board[yy][x] = board[yy - 1][x];
            }
        }
        for (int x = 0; x < BOARD_W; x++) {
            board[0][x] = 0;
        }
        y++;
    }

    return cleared;
}

static void spawn_piece(void)
{
    current_piece.type = next_piece;
    current_piece.rotation = 0;
    current_piece.x = 3;
    current_piece.y = -1;
    next_piece = random_piece();

    if (collides(&current_piece, current_piece.x, current_piece.y, current_piece.rotation)) {
        state = STATE_GAME_OVER;
    }
}

static void apply_score(uint32_t cleared)
{
    static const uint32_t table[5] = {0, 100, 300, 500, 800};

    if (cleared == 0U) {
        return;
    }

    score += table[cleared] * level;
    lines_cleared += cleared;
    level = (lines_cleared / 10U) + 1U;
}

static void lock_piece(void)
{
    uint16_t shape = shapes[current_piece.type][current_piece.rotation & 3];
    uint8_t color = piece_colors[current_piece.type];

    for (int y = 0; y < 4; y++) {
        for (int x = 0; x < 4; x++) {
            if (!shape_cell(shape, x, y)) {
                continue;
            }

            int bx = current_piece.x + x;
            int by = current_piece.y + y;
            if (bx >= 0 && bx < BOARD_W && by >= 0 && by < BOARD_H) {
                board[by][bx] = color;
            }
        }
    }

    apply_score(compact_lines());
    spawn_piece();
}

static void reset_game(enum game_state new_state)
{
    clear_board();
    score = 0;
    lines_cleared = 0;
    level = 1;
    rng_state = timer_get_ticks() ^ 0x1BADB002U;
    next_piece = random_piece();
    last_drop_tick = timer_get_ticks();
    game_start_tick = last_drop_tick;
    state = new_state;

    if (new_state == STATE_PLAYING) {
        spawn_piece();
    }
}

static void hard_drop(void)
{
    while (move_piece(0, 1)) {
        score += 2U;
    }
    lock_piece();
}

static void soft_drop(void)
{
    if (move_piece(0, 1)) {
        score++;
    } else {
        lock_piece();
    }
    last_drop_tick = timer_get_ticks();
}

static bool update_game_tick(void)
{
    uint32_t now = timer_get_ticks();
    if ((now - last_drop_tick) >= drop_interval()) {
        if (!move_piece(0, 1)) {
            lock_piece();
        }
        last_drop_tick = now;
        return true;
    }
    return false;
}

static void handle_key(int key)
{
    if (state == STATE_LAYOUT) {
        if (key == 'g' || key == 'G') {
            keyboard_set_layout(KEYBOARD_LAYOUT_DE);
            speaker_music_start();
            reset_game(STATE_TITLE);
        } else if (key == 'u' || key == 'U') {
            keyboard_set_layout(KEYBOARD_LAYOUT_US);
            speaker_music_start();
            reset_game(STATE_TITLE);
        }
        return;
    }

    if (state == STATE_TITLE) {
        if (key == KEY_ENTER) {
            reset_game(STATE_PLAYING);
        }
        return;
    }

    if (state == STATE_GAME_OVER) {
        if (key == KEY_ENTER) {
            reset_game(STATE_PLAYING);
        }
        return;
    }

    if (key == KEY_ESCAPE) {
        speaker_music_stop();
        reset_game(STATE_LAYOUT);
        return;
    }

    if (key == 'p' || key == 'P') {
        state = (state == STATE_PAUSED) ? STATE_PLAYING : STATE_PAUSED;
        return;
    }

    if (state != STATE_PLAYING) {
        return;
    }

    if (key == 'a' || key == 'A' || key == KEY_LEFT) {
        (void)move_piece(-1, 0);
    } else if (key == 'd' || key == 'D' || key == KEY_RIGHT) {
        (void)move_piece(1, 0);
    } else if (key == 's' || key == 'S' || key == KEY_DOWN) {
        soft_drop();
    } else if (key == 'w' || key == 'W' || key == KEY_UP) {
        (void)rotate_piece();
    } else if (key == ' ') {
        hard_drop();
    }
}

static void draw_cell(int x, int y, uint8_t color)
{
    int px = BOARD_X + (x * CELL);
    int py = BOARD_Y + (y * CELL);
    vga_fill_rect(px, py, CELL, CELL, color);
    vga_fill_rect(px, py, CELL, 1, 0);
    vga_fill_rect(px, py, 1, CELL, 0);
}

static void draw_piece_on_board(const struct piece *piece)
{
    uint16_t shape = shapes[piece->type][piece->rotation & 3];
    uint8_t color = piece_colors[piece->type];

    for (int y = 0; y < 4; y++) {
        for (int x = 0; x < 4; x++) {
            if (!shape_cell(shape, x, y)) {
                continue;
            }

            int bx = piece->x + x;
            int by = piece->y + y;
            if (bx >= 0 && bx < BOARD_W && by >= 0 && by < BOARD_H) {
                draw_cell(bx, by, color);
            }
        }
    }
}

static void draw_board(void)
{
    for (int y = 0; y < BOARD_H; y++) {
        for (int x = 0; x < BOARD_W; x++) {
            uint8_t color = board[y][x] == 0U ? 8U : board[y][x];
            draw_cell(x, y, color);
        }
    }

    if (state == STATE_PLAYING || state == STATE_PAUSED) {
        draw_piece_on_board(&current_piece);
    }

    vga_fill_rect(BOARD_X, BOARD_Y, BOARD_W * CELL, 1, 7);
    vga_fill_rect(BOARD_X, BOARD_Y + ((BOARD_H * CELL) - 1), BOARD_W * CELL, 1, 7);
    vga_fill_rect(BOARD_X, BOARD_Y, 1, BOARD_H * CELL, 7);
    vga_fill_rect(BOARD_X + ((BOARD_W * CELL) - 1), BOARD_Y, 1, BOARD_H * CELL, 7);
}

static void draw_stats(void)
{
    vga_draw_string(8, 16, "SCORE", 7, 0);
    vga_draw_number(8, 28, score, 14, 0);
    vga_draw_string(8, 52, "LEVEL", 7, 0);
    vga_draw_number(8, 64, level, 14, 0);
    vga_draw_string(8, 88, "LINES", 7, 0);
    vga_draw_number(8, 100, lines_cleared, 14, 0);
}

static void draw_next(void)
{
    uint16_t shape = shapes[next_piece][0];
    uint8_t color = piece_colors[next_piece];

    vga_draw_string(244, 32, "NEXT", 7, 0);
    vga_fill_rect(PREVIEW_X - 5, PREVIEW_Y - 5, 50, 50, 7);
    vga_fill_rect(PREVIEW_X - 4, PREVIEW_Y - 4, 48, 48, 0);

    for (int y = 0; y < 4; y++) {
        for (int x = 0; x < 4; x++) {
            if (shape_cell(shape, x, y)) {
                vga_fill_rect(PREVIEW_X + (x * 10), PREVIEW_Y + (y * 10), 9, 9, color);
            }
        }
    }
}

static void draw_center_message(const char *line1, const char *line2)
{
    vga_fill_rect(56, 78, 208, 44, 0);
    vga_draw_string(80, 86, line1, 14, 0);
    vga_draw_string(72, 104, line2, 7, 0);
}

static void render(void)
{
    vga_clear(0);

    if (state == STATE_LAYOUT) {
        vga_draw_string(56, 56, "KEYBOARD LAYOUT", 14, 0);
        vga_draw_string(72, 92, "G  GERMAN", 7, 0);
        vga_draw_string(72, 112, "U  US", 7, 0);
        return;
    }

    if (state == STATE_TITLE) {
        vga_draw_string(96, 64, "TETRISOS", 14, 0);
        vga_draw_string(72, 96, "PRESS ENTER", 7, 0);
        vga_draw_string(88, 112, "TO START", 7, 0);
        return;
    }

    draw_stats();
    draw_board();
    draw_next();

    if (state == STATE_PAUSED) {
        draw_center_message("PAUSED", "PRESS P");
    } else if (state == STATE_GAME_OVER) {
        draw_center_message("GAME OVER", "ENTER RESTART");
        vga_draw_string(80, 126, "FINAL", 7, 0);
        vga_draw_number(136, 126, score, 14, 0);
    }
}

void tetris_run(void)
{
    uint32_t last_render_tick = 0;
    bool dirty = true;

    reset_game(STATE_LAYOUT);

    for (;;) {
        speaker_music_update();

        int key = keyboard_get_key();
        if (key != KEY_NONE) {
            handle_key(key);
            dirty = true;
        }

        if (state == STATE_PLAYING) {
            if (update_game_tick()) {
                dirty = true;
            }
        }

        uint32_t now = timer_get_ticks();
        if (dirty || (now - last_render_tick) >= 3U) {
            render();
            vga_present();
            last_render_tick = now;
            dirty = false;
        }

#if defined(__x86_64__) || defined(__i386__)
        __asm__ volatile ("hlt");
#elif defined(__riscv)
        __asm__ volatile ("wfi");
#endif
    }
}

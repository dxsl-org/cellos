#include <stdint.h>

// We need to provide these types
#define MP_SSIZE_MAX (0x7fffffff)

// Type definitions for specific bit-widths
typedef int32_t mp_int_t; 
typedef uint32_t mp_uint_t;
typedef long mp_off_t;

// We use POSIX shim for IO
#include <stdio.h>

// Feature configuration
#define MICROPY_OBJ_REPR            (MICROPY_OBJ_REPR_D) // 64-bit float usually
#define MICROPY_NLR_SETJMP          (1)
#define MICROPY_ENABLE_COMPILER     (1)
#define MICROPY_ENABLE_GC           (1)
#define MICROPY_HELPER_REPL         (1)
#define MICROPY_LONGINT_IMPL        (MICROPY_LONGINT_IMPL_MPZ)
#define MICROPY_FLOAT_IMPL          (MICROPY_FLOAT_IMPL_DOUBLE)
#define MICROPY_ENABLE_SOURCE_LINE  (1)

// QSTR Management: We disable it or map to static if possible to avoid python dependency in build
// However, MicroPython core really needs qstrs. 
// We will rely on a pre-generated or assume NO_QSTR won't work.
// For now, let's just minimal config.

// Hal functions
#define mp_hal_stdin_rx_chr() getchar()
#define mp_hal_stdout_tx_strn(str, len) fwrite(str, 1, len, stdout)

// Entry point macros
#define MICROPY_PORT_INIT_FUNC      // empty
#define MICROPY_PORT_DEINIT_FUNC    // empty
#define MICROPY_PORT_ROOT_POINTERS  // empty

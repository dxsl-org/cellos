#include <cellos_platform.h>

/* Called by the Rust Cell host after it has installed the platform callback. */
int cellos_porting_smoke(void) {
    return cellos_time_ms() == 0 ? 1 : 0;
}

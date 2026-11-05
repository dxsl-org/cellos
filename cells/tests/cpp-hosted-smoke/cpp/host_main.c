// SPDX-License-Identifier: MPL-2.0
// Host driver for the `cpp-hosted` witness logic (scripts/build-llvm-runtimes.sh
// host row). Mirrors src/main.rs: run the checks, then end in the uncaught
// exception, which must terminate the process with libc++abi's message.
#include <stdio.h>

int cpp_hosted_witness(void);
void cpp_hosted_uncaught(void);

int main(void) {
    if (cpp_hosted_witness() != 0)
        return 1;
    cpp_hosted_uncaught();
    printf("CPP-HOSTED: uncaught FAIL (returned)\n");
    return 1;
}

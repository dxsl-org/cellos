# Cellos x86_64 Tier-2 hard-float C compilation toolchain (runtime plan D-R3,
# rule H1). Rust owns final cell linking.
set(CMAKE_SYSTEM_NAME Generic)
set(CMAKE_SYSTEM_PROCESSOR x86_64)
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
get_filename_component(CELLOS_ROOT "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)
set(CMAKE_C_COMPILER "${CELLOS_ROOT}/tools/cellos-cc")
set(CMAKE_C_COMPILER_TARGET x86_64-unknown-none-elf)
# tools/cellos-cc picks the compiler and flags from CELLOS_C_TARGET. The ENV
# setting covers configure-time compiler detection only; the launcher carries
# it into every build-time compile, so a caller's environment can never select
# the no-SSE x86 row or another architecture for hard-float Tier-2 objects.
set(ENV{CELLOS_C_TARGET} x86_64-unknown-none-sse2)
set(CMAKE_C_COMPILER_LAUNCHER env CELLOS_C_TARGET=x86_64-unknown-none-sse2)
set(CMAKE_C_FLAGS_INIT "-std=c11")
set(CMAKE_EXE_LINKER_FLAGS_INIT "")

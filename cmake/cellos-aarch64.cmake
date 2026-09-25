# Cellos AArch64 C compilation toolchain. Rust owns final cell linking.
set(CMAKE_SYSTEM_NAME Generic)
set(CMAKE_SYSTEM_PROCESSOR aarch64)
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
get_filename_component(CELLOS_ROOT "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)
set(CMAKE_C_COMPILER "${CELLOS_ROOT}/tools/cellos-cc")
set(CMAKE_C_COMPILER_TARGET aarch64-unknown-none-elf)
# tools/cellos-cc selects the compiler and arch flags from this variable; without
# it the wrapper falls back to its RV64 default and ships a riscv64 object into an
# aarch64 link (`smoke.c.obj is incompatible with symbols.o`).
set(ENV{CELLOS_C_TARGET} aarch64-unknown-none-softfloat)
set(CMAKE_C_FLAGS_INIT "-std=c11")
set(CMAKE_EXE_LINKER_FLAGS_INIT "")

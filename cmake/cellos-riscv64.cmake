# Cellos RV64 C compilation toolchain. Rust owns final cell linking.
set(CMAKE_SYSTEM_NAME Generic)
set(CMAKE_SYSTEM_PROCESSOR riscv64)
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
get_filename_component(CELLOS_ROOT "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)
set(CMAKE_C_COMPILER "${CELLOS_ROOT}/tools/cellos-cc")
set(CMAKE_C_COMPILER_TARGET riscv64-unknown-none-elf)
# tools/cellos-cc reads this to pick the real compiler; set it here rather than
# relying on the wrapper's default so a caller's environment cannot leak another
# architecture into an RV64 link.
set(ENV{CELLOS_C_TARGET} riscv64gc-unknown-none-elf)
set(CMAKE_C_FLAGS_INIT "-std=c11")
set(CMAKE_EXE_LINKER_FLAGS_INIT "")

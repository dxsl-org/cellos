#!/usr/bin/env bash
# build-llvm-runtimes.sh — Build the `cpp-hosted` C++ runtime (ADR-0034):
# static LLVM libc++, libc++abi and libunwind, exceptions and RTTI on.
#
# Flow:
#   1. Fetch llvm-project at LLVM_COMMIT (tag LLVM_TAG), sparse: runtimes,
#      libcxx, libcxxabi, libunwind and the CMake modules they need, into
#      target/llvm-runtimes/src (gitignored).
#   2. Licence check, per file, of every file under the include/ and src/ trees
#      that can reach a Cell (third_party/llvm-runtimes/README.md).
#   3. host:   dry run for x86_64-linux-gnu with the same option set, then a
#              host run of the witness logic (cells/tests/cpp-hosted-smoke/cpp).
#   4. x86_64: cross build over the mlibc `posix` sysroot
#              (third_party/mlibc/build-x86_64, from scripts/build-mlibc.sh) with
#              cmake/cellos-x86_64.cmake; installs into target/llvm-runtimes/x86_64,
#              then requires every libc symbol the archives use to exist in
#              mlibc's libc.a (a gap is fixed in mlibc, never stubbed here).
#   5. size:   ADR-0034 size table. Links the witness logic and a Mesa-shaped
#              probe (-fno-exceptions -fno-rtti) as static PIEs over mlibc with
#              --gc-sections and attributes the kept input-section bytes to
#              libc++.a, libc++abi.a and libunwind.a from the link map.
#
# Pin: llvmorg-19.1.7, the newest release whose libc++ lists clang 18 (the
# Ubuntu 24.04 compiler every Cellos C/C++ row uses) as supported; libc++ 20
# drops it. Moving the pin is a deliberate edit: rerun the licence check, both
# rows, the witness link proof and the size table in ADR-0034.
#
# Usage: bash scripts/build-llvm-runtimes.sh [host|x86_64|size|all]   (default: all)
# Env:   LLVM_RUNTIMES_JOBS (ninja -j), CLANG (default clang), CLANGXX (clang++),
#        MLIBC_X86_64_DIR (default third_party/mlibc/build-x86_64)
set -euo pipefail

LLVM_TAG="llvmorg-19.1.7"
LLVM_COMMIT="cd708029e0b2869e80abe31ddb175f7c35361f90"
LLVM_REPO="https://github.com/llvm/llvm-project.git"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$REPO_ROOT/target/llvm-runtimes"
SRC="$OUT/src"
MLIBC_DIR="${MLIBC_X86_64_DIR:-$REPO_ROOT/third_party/mlibc/build-x86_64}"
MLIBC_INCLUDE="$MLIBC_DIR/sysroot/include"
CLANG="${CLANG:-clang}"
CLANGXX="${CLANGXX:-clang++}"
JOBS=()
[[ -n "${LLVM_RUNTIMES_JOBS:-}" ]] && JOBS=(-j "$LLVM_RUNTIMES_JOBS")
ROW="${1:-all}"
case "$ROW" in host|x86_64|size|all) ;; *) echo "usage: $0 [host|x86_64|size|all]" >&2; exit 64 ;; esac

for tool in git cmake ninja "$CLANG" "$CLANGXX"; do
    command -v "$tool" >/dev/null 2>&1 || { echo "FAIL: $tool not found" >&2; exit 1; }
done

# ─── Step 1: llvm-project source at the pin ──────────────────────────────────
echo "=== Step 1: llvm-project $LLVM_TAG @ $LLVM_COMMIT ==="
if [[ ! -d "$SRC/.git" ]]; then
    git init -q "$SRC"
    git -C "$SRC" remote add origin "$LLVM_REPO"
fi
git -C "$SRC" sparse-checkout set --no-cone \
    /LICENSE.TXT /runtimes/ /libcxx/ /libcxxabi/ /libunwind/ /cmake/ /llvm/cmake/ /llvm/utils/llvm-lit/ /llvm/utils/lit/
if ! git -C "$SRC" cat-file -e "$LLVM_COMMIT^{commit}" 2>/dev/null; then
    git -C "$SRC" fetch -q --depth 1 --filter=blob:none origin "refs/tags/$LLVM_TAG"
    fetched="$(git -C "$SRC" rev-parse 'FETCH_HEAD^{commit}')"
    [[ "$fetched" == "$LLVM_COMMIT" ]] \
        || { echo "FAIL: tag $LLVM_TAG resolves to $fetched, pin is $LLVM_COMMIT" >&2; exit 1; }
fi
git -C "$SRC" checkout -q -f "$LLVM_COMMIT"
echo "  llvm-project commit: $(git -C "$SRC" rev-parse HEAD)"

# ─── Step 2: per-file licence check ──────────────────────────────────────────
# Every file under these trees is either compiled into a Cell or installed as a
# header Cells compile against. Each must carry the LLVM SPDX line, or be
# build metadata, or be one of the three one-line init-priority fragments that
# libc++ #includes into SPDX-tagged .cpp files (README.md lists them).
echo "=== Step 2: licence check ==="
SPDX="SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception"
UNTAGGED_FRAGMENTS=(
    libcxx/src/iostream_init.h
    libcxx/src/chrono_system_time_init.h
    libcxx/src/memory_resource_init_helper.h
)
for lic in libcxx/LICENSE.TXT libcxxabi/LICENSE.TXT libunwind/LICENSE.TXT; do
    grep -q "Apache License v2.0 with LLVM Exceptions" "$SRC/$lic" \
        || { echo "FAIL: $lic is not Apache-2.0 with LLVM Exceptions" >&2; exit 1; }
done
checked=0
bad=()
while IFS= read -r -d '' file; do
    rel="${file#"$SRC"/}"
    checked=$((checked + 1))
    grep -q "$SPDX" "$file" && continue
    case "$rel" in
        */CMakeLists.txt | *.modulemap | *.txt | *.in | */.clang-format | */.clang-tidy) continue ;;
        libcxxabi/src/demangle/cp-to-llvm.sh) continue ;;
    esac
    for fragment in "${UNTAGGED_FRAGMENTS[@]}"; do
        if [[ "$rel" == "$fragment" ]]; then
            [[ "$(grep -cv '^#pragma GCC system_header$' "$file")" -le 1 ]] \
                || { echo "FAIL: $rel grew beyond a one-line fragment; re-review" >&2; exit 1; }
            continue 2
        fi
    done
    bad+=("$rel")
done < <(find "$SRC"/libcxx/{include,src} "$SRC"/libcxxabi/{include,src} \
              "$SRC"/libunwind/{include,src} -type f -print0)
if [[ ${#bad[@]} -gt 0 ]]; then
    printf 'FAIL: no LLVM SPDX line: %s\n' "${bad[@]}" >&2
    exit 1
fi
# libc++'s Ryu files add the Boost-1.0 notice of Ulf Adams beside the SPDX line;
# NOTICE carries it.
ryu="$(grep -rl "Copyright 2018 Ulf Adams" "$SRC/libcxx" | wc -l)"
echo "  $checked files: LLVM SPDX or build metadata; $ryu Ryu files also carry Boost-1.0 (Ulf Adams)"

# ─── Common option set (both rows) ───────────────────────────────────────────
# Every disabled feature here is listed in the profile contract
# (docs/specs/05-application.md §3.2, third_party/llvm-runtimes/README.md).
COMMON_OPTS=(
    -G Ninja
    -DCMAKE_BUILD_TYPE=Release
    -DLLVM_ENABLE_RUNTIMES="libcxx;libcxxabi;libunwind"
    -DLLVM_INCLUDE_TESTS=OFF
    -DLLVM_INCLUDE_DOCS=OFF
    -DLIBCXX_INCLUDE_TESTS=OFF
    -DLIBCXX_INCLUDE_BENCHMARKS=OFF
    -DLIBCXX_INCLUDE_DOCS=OFF
    -DLIBCXXABI_INCLUDE_TESTS=OFF
    -DLIBUNWIND_INCLUDE_TESTS=OFF
    -DLIBUNWIND_INCLUDE_DOCS=OFF
    # Static only.
    -DLIBCXX_ENABLE_SHARED=OFF
    -DLIBCXXABI_ENABLE_SHARED=OFF
    -DLIBUNWIND_ENABLE_SHARED=OFF
    -DLIBCXX_ENABLE_STATIC=ON
    -DLIBCXXABI_ENABLE_STATIC=ON
    -DLIBUNWIND_ENABLE_STATIC=ON
    # Three separate archives, so each one's size is measurable.
    -DLIBCXX_ENABLE_STATIC_ABI_LIBRARY=OFF
    -DLIBCXXABI_USE_LLVM_UNWINDER=ON
    -DLIBCXXABI_ENABLE_STATIC_UNWINDER=ON
    -DLIBCXXABI_STATICALLY_LINK_UNWINDER_IN_STATIC_LIBRARY=OFF
    # One build, exceptions and RTTI on; TUs may still compile without them.
    -DLIBCXX_ENABLE_EXCEPTIONS=ON
    -DLIBCXX_ENABLE_RTTI=ON
    -DLIBCXXABI_ENABLE_EXCEPTIONS=ON
    # The terminate handler names the uncaught exception before abort().
    -DLIBCXXABI_SILENT_TERMINATE=OFF
    # Threads on mlibc pthread (R3a); the target triple is neither Linux nor
    # __unix__, so libc++ cannot detect the API by itself.
    -DLIBCXX_ENABLE_THREADS=ON
    -DLIBCXX_HAS_PTHREAD_API=ON
    -DLIBCXXABI_ENABLE_THREADS=ON
    -DLIBCXXABI_HAS_PTHREAD_API=ON
    -DLIBUNWIND_ENABLE_THREADS=ON
    -DLIBCXX_ENABLE_MONOTONIC_CLOCK=ON
    # llama.cpp: iostreams, std::wregex, codecvt.
    -DLIBCXX_ENABLE_LOCALIZATION=ON
    -DLIBCXX_ENABLE_WIDE_CHARACTERS=ON
    -DLIBCXX_ENABLE_UNICODE=ON
    -DLIBCXX_ENABLE_FILESYSTEM=ON
    -DLIBCXX_ENABLE_RANDOM_DEVICE=ON
    # Disabled: tzdb needs a zoneinfo file tree; the experimental library is
    # not part of the C++17 contract; no C++20 module sources are shipped.
    -DLIBCXX_ENABLE_TIME_ZONE_DATABASE=OFF
    -DLIBCXX_ENABLE_EXPERIMENTAL_LIBRARY=OFF
    -DLIBCXX_INSTALL_MODULES=OFF
    # Compiler builtins are not taken from compiler-rt: the Cell link already
    # carries Rust compiler_builtins and mlibc's libgcc.a (ADR-0034 Consequences).
    -DLIBCXX_USE_COMPILER_RT=OFF
    -DLIBCXXABI_USE_COMPILER_RT=OFF
    -DLIBUNWIND_USE_COMPILER_RT=OFF
)
# Every object may be garbage-collected per function by the Cell link.
SECTION_FLAGS="-ffunction-sections -fdata-sections"

# run_cmake <build dir> <install dir> <cmake args...>
run_cmake() {
    local build="$1" install="$2"
    shift 2
    rm -rf "$build" "$install"
    cmake -S "$SRC/runtimes" -B "$build" "${COMMON_OPTS[@]}" \
        -DCMAKE_INSTALL_PREFIX="$install" "$@" >"$build.configure.log" 2>&1 \
        || { tail -40 "$build.configure.log" >&2; echo "FAIL: configure, see $build.configure.log" >&2; exit 1; }
    ninja -C "$build" "${JOBS[@]}" install-cxx install-cxxabi install-unwind >"$build.build.log" 2>&1 \
        || { grep -m20 -B2 -A6 "error" "$build.build.log" >&2 || tail -40 "$build.build.log" >&2
             echo "FAIL: build, see $build.build.log" >&2; exit 1; }
    for lib in libc++.a libc++abi.a libunwind.a; do
        [[ -s "$install/lib/$lib" ]] || { echo "FAIL: $install/lib/$lib not produced" >&2; exit 1; }
        echo "  $lib: $(stat -c %s "$install/lib/$lib") bytes"
    done
}

# ─── Step 3: host dry run ────────────────────────────────────────────────────
if [[ "$ROW" == host || "$ROW" == all ]]; then
    echo "=== Step 3: host dry run (x86_64-linux-gnu) ==="
    HOST_INSTALL="$OUT/host"
    run_cmake "$OUT/build-host" "$HOST_INSTALL" \
        -DCMAKE_C_COMPILER="$CLANG" -DCMAKE_CXX_COMPILER="$CLANGXX" \
        -DCMAKE_C_FLAGS="$SECTION_FLAGS" -DCMAKE_CXX_FLAGS="$SECTION_FLAGS"
    # The witness logic on the host build: the same C++ the Cell runs, driven by
    # a C main, so option or library errors surface without Cellos.
    WITNESS_CPP="$REPO_ROOT/cells/tests/cpp-hosted-smoke/cpp"
    "$CLANGXX" -std=c++17 -O2 -nostdinc++ -isystem "$HOST_INSTALL/include/c++/v1" \
        -c "$WITNESS_CPP/witness.cpp" -o "$OUT/host-witness.o"
    "$CLANG" -O2 -c "$WITNESS_CPP/host_main.c" -o "$OUT/host-main.o"
    # Dynamic glibc: a -static link would also pull libgcc_eh's unwinder, which
    # duplicates libunwind.a.
    "$CLANGXX" -nostdlib++ "$OUT/host-main.o" "$OUT/host-witness.o" \
        "$HOST_INSTALL/lib/libc++.a" "$HOST_INSTALL/lib/libc++abi.a" "$HOST_INSTALL/lib/libunwind.a" \
        -lpthread -o "$OUT/host-witness"
    grep -q " T _Unwind_RaiseException$" <<<"$(nm "$OUT/host-witness")" \
        || { echo "FAIL: host witness does not carry libunwind's _Unwind_RaiseException" >&2; exit 1; }
    set +e
    "$OUT/host-witness" >"$OUT/host-witness.log" 2>&1
    status=$?
    set -e
    sed 's/^/  | /' "$OUT/host-witness.log"
    grep -q "^CPP-HOSTED: [0-9]*/[0-9]* pass$" "$OUT/host-witness.log" \
        && ! grep -q "FAIL" "$OUT/host-witness.log" \
        && grep -q "terminating due to uncaught exception of type std::runtime_error: cpp-hosted uncaught" "$OUT/host-witness.log" \
        && [[ $status -ne 0 ]] \
        || { echo "FAIL: host witness (exit $status), see $OUT/host-witness.log" >&2; exit 1; }
    echo "  host witness: markers OK, uncaught exception terminated the process (exit $status)"
fi

# ─── Step 4: x86_64 Cellos over mlibc ────────────────────────────────────────
if [[ "$ROW" == x86_64 || "$ROW" == all ]]; then
    echo "=== Step 4: x86_64 Cellos (mlibc posix) ==="
    [[ -d "$MLIBC_INCLUDE" ]] || { echo "FAIL: $MLIBC_INCLUDE missing; run: bash scripts/build-mlibc.sh" >&2; exit 1; }
    BUILD="$OUT/build-x86_64"
    mkdir -p "$OUT"
    # R1's toolchain owns the C compiler (tools/cellos-cc, SSE2 row, rule H1).
    # It sets no C++ or assembler entry, so this wrapper adds clang++ and clang
    # with the same target and float contract, over mlibc's headers only: no
    # /usr/include (-nostdlibinc); clang's resource headers stay.
    TOOLCHAIN="$OUT/cellos-x86_64-cpp-hosted.cmake"
    cat >"$TOOLCHAIN" <<EOF
include("$REPO_ROOT/cmake/cellos-x86_64.cmake")
set(CMAKE_CXX_COMPILER "$(command -v "$CLANGXX")")
set(CMAKE_CXX_COMPILER_TARGET x86_64-unknown-none-elf)
set(CMAKE_ASM_COMPILER "$(command -v "$CLANG")")
set(CMAKE_ASM_COMPILER_TARGET x86_64-unknown-none-elf)
set(CMAKE_AR "$(command -v llvm-ar-18 || command -v llvm-ar || command -v ar)")
set(CMAKE_RANLIB "$(command -v llvm-ranlib-18 || command -v llvm-ranlib || command -v ranlib)")
EOF
    # _GNU_SOURCE: what clang++ predefines for C++ on GNU targets; it opens
    # mlibc's POSIX-2024 surface (dladdr, *_l locale functions) to libc++.
    LIBC_FLAGS="-D_GNU_SOURCE -nostdlibinc -isystem $MLIBC_INCLUDE"
    CXX_FLAGS="-msse2 -mfpmath=sse -mno-red-zone -mcmodel=small -fPIC -fno-stack-protector $LIBC_FLAGS $SECTION_FLAGS"
    # The configure-time library probes only compile (static try-compile), so
    # they all "succeed" and would tag objects with -lpthread/-ldl/-lrt. mlibc's
    # libc.a carries pthread, dl and rt; the Rust Cell link supplies libc.a
    # and libgcc.a (libs/mlibc-shim).
    NO_LIB_PROBES=()
    for probe in LIBCXX_HAS_{PTHREAD,RT,ATOMIC,GCC_S}_LIB \
                 LIBCXXABI_HAS_{PTHREAD,DL,C,GCC,GCC_S}_LIB \
                 LIBUNWIND_HAS_{PTHREAD,DL,C,GCC,GCC_S}_LIB; do
        NO_LIB_PROBES+=("-D$probe=OFF")
    done
    # mlibc's <ctype.h> exposes none of the classification tables libc++
    # knows (glibc, BSD, newlib), so libc++ uses its own C-locale table, as for
    # musl. __config_site carries the choice to every consumer TU.
    run_cmake "$BUILD" "$OUT/x86_64" \
        -DCMAKE_TOOLCHAIN_FILE="$TOOLCHAIN" \
        -DCMAKE_C_FLAGS="$LIBC_FLAGS $SECTION_FLAGS" \
        -DCMAKE_ASM_FLAGS="-fPIC $LIBC_FLAGS" \
        -DCMAKE_CXX_FLAGS="$CXX_FLAGS" \
        -DLIBCXX_HAS_MUSL_LIBC=OFF \
        -DLIBUNWIND_IS_BAREMETAL=OFF \
        -DLIBCXX_EXTRA_SITE_DEFINES="_LIBCPP_PROVIDES_DEFAULT_RUNE_TABLE" \
        "${NO_LIB_PROBES[@]}"
    # No `#pragma comment(lib, ...)` may reach a Cell: lld would look for
    # -lpthread/-ldl, which the Rust Cell link does not have.
    if readelf -SW "$OUT"/x86_64/lib/lib{c++,c++abi,unwind}.a | grep -q "\.deplibs"; then
        echo "FAIL: a runtime archive carries .deplibs (library probe leaked)" >&2
        exit 1
    fi
    # The libc symbols the runtime needs from mlibc: G3's link inventory input.
    nm -u "$OUT"/x86_64/lib/lib{c++,c++abi,unwind}.a 2>/dev/null | awk 'NF == 2 { print $2 }' | sort -u \
        >"$OUT/x86_64/undefined-symbols.txt"
    echo "  undefined symbols across the three archives: $(wc -l <"$OUT/x86_64/undefined-symbols.txt")" \
         "(target/llvm-runtimes/x86_64/undefined-symbols.txt)"
    # Each must be defined by the runtime itself, mlibc's libc.a or the libgcc.a
    # shipped beside it. __dso_handle is defined by lld at the Cell link.
    nm --defined-only "$OUT"/x86_64/lib/lib{c++,c++abi,unwind}.a "$MLIBC_DIR/libc.a" "$MLIBC_DIR/libgcc.a" 2>/dev/null \
        | awk 'NF == 3 { print $3 }' | sort -u >"$OUT/x86_64/defined-symbols.txt"
    missing="$(comm -23 "$OUT/x86_64/undefined-symbols.txt" "$OUT/x86_64/defined-symbols.txt" | grep -vx "__dso_handle" || true)"
    if [[ -n "$missing" ]]; then
        printf 'FAIL: not defined by mlibc (%s): %s\n' "$MLIBC_DIR" "$(tr '\n' ' ' <<<"$missing")" >&2
        exit 1
    fi
    echo "  every runtime libc reference resolves in $MLIBC_DIR/libc.a"
fi

# ─── Step 5: size table (ADR-0034) ───────────────────────────────────────────
if [[ "$ROW" == size || "$ROW" == all ]]; then
    echo "=== Step 5: size table ==="
    RT="$OUT/x86_64"
    [[ -s "$RT/lib/libc++.a" ]] || { echo "FAIL: $RT missing; run the x86_64 row first" >&2; exit 1; }
    LLD="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/rust-lld"
    [[ -x "$LLD" ]] || { echo "FAIL: rust-lld not found at $LLD" >&2; exit 1; }
    STRIP="$(command -v llvm-strip-18 || command -v llvm-strip || command -v strip)"
    SIZE_DIR="$OUT/size"
    rm -rf "$SIZE_DIR"
    mkdir -p "$SIZE_DIR"
    # The `cpp-hosted` consumer flags (README.md "Profile contract").
    PROFILE=(--target=x86_64-unknown-none-elf -std=c++17 -O2 -msse2 -mfpmath=sse -mno-red-zone
             -mcmodel=small -fPIC -fno-stack-protector -ffunction-sections -fdata-sections
             -D_GNU_SOURCE -nostdinc++ -nostdlibinc -isystem "$RT/include/c++/v1" -isystem "$MLIBC_INCLUDE")
    WITNESS_CPP="$REPO_ROOT/cells/tests/cpp-hosted-smoke/cpp"
    "$CLANGXX" "${PROFILE[@]}" -c "$WITNESS_CPP/witness.cpp" -o "$SIZE_DIR/witness.o"
    "$CLANGXX" "${PROFILE[@]}" -fno-exceptions -fno-rtti -c "$WITNESS_CPP/mesa_probe.cpp" -o "$SIZE_DIR/mesa.o"
    # attribute <map>: "<archive> <file-backed bytes> <NOBITS bytes>" for every
    # archive with kept input sections. lld lists .eh_frame and .eh_frame_hdr
    # as linker-internal, so unwind tables are reported for the whole image.
    attribute() {
        awk 'NR == 1 { out_col = index($0, "Out"); in_col = index($0, "In"); next }
             { col = index($0, $5) }
             col == out_col { out = $5; next }
             col != in_col || $5 !~ /\.a\(/ || out ~ /^\.(debug|comment|symtab|strtab|shstrtab)/ { next }
             { path = $5; sub(/\(.*/, "", path); n = split(path, parts, "/")
               size = strtonum("0x" $3)
               if (out ~ /^\.t?bss/) nobits[parts[n]] += size; else bytes[parts[n]] += size }
             END { for (lib in bytes) printf "%s %d %d\n", lib, bytes[lib], nobits[lib] }' "$1" | sort
    }
    printf '  %-12s %10s %10s %10s %10s %10s %12s\n' link libc++ libc++abi libunwind runtime nobits stripped-elf
    for probe in witness:cpp_hosted_witness:cpp_hosted_uncaught mesa:cpp_hosted_mesa_probe:cpp_hosted_mesa_probe; do
        IFS=: read -r name entry keep <<<"$probe"
        "$LLD" -flavor gnu -pie --no-dynamic-linker --gc-sections --eh-frame-hdr -e "$entry" -u "$keep" \
            -Map "$SIZE_DIR/$name.map" -o "$SIZE_DIR/$name.elf" "$SIZE_DIR/$name.o" \
            "$RT/lib/libc++.a" "$RT/lib/libc++abi.a" "$RT/lib/libunwind.a" "$MLIBC_DIR/libc.a" "$MLIBC_DIR/libgcc.a"
        "$STRIP" -o "$SIZE_DIR/$name.stripped.elf" "$SIZE_DIR/$name.elf"
        attribute "$SIZE_DIR/$name.map" >"$SIZE_DIR/$name.attr"
        lib_bytes() { awk -v lib="$1" -v col="$2" '$1 == lib { print $col; found = 1 } END { if (!found) print 0 }' "$SIZE_DIR/$name.attr"; }
        cxx="$(lib_bytes libc++.a 2)"; abi="$(lib_bytes libc++abi.a 2)"; unw="$(lib_bytes libunwind.a 2)"
        nobits=$(($(lib_bytes libc++.a 3) + $(lib_bytes libc++abi.a 3) + $(lib_bytes libunwind.a 3)))
        printf '  %-12s %10d %10d %10d %10d %10d %12d\n' "$name" "$cxx" "$abi" "$unw" $((cxx + abi + unw)) \
            "$nobits" "$(stat -c %s "$SIZE_DIR/$name.stripped.elf")"
        eh="$(readelf -SW "$SIZE_DIR/$name.elf" | sed 's/^ *\[ *[0-9]*\] *//' \
            | awk '$1 == ".eh_frame" || $1 == ".eh_frame_hdr" { s += strtonum("0x" $5) } END { print s + 0 }')"
        echo "  $name: whole-image .eh_frame + .eh_frame_hdr = $eh bytes"
    done
    echo "  maps and stripped ELFs: ${SIZE_DIR#"$REPO_ROOT"/}"
fi

echo ""
echo "LLVM runtimes complete ($LLVM_TAG, $ROW)."

# Generate disk images for ViCell.
#
# Two separate images are produced:
#
#   kernel/src/embedded/kernel_fs.img  (~8 MB, FAT32)
#       Embedded in the kernel binary via ramdisk.rs (include_bytes!).
#       Contains release-built cell ELFs + /hostname + /readme.
#       Served by the kernel's internal filesystem (sys_open / ReadDir).
#
#   disk_v3.img  (~455 MB, MBR — see tools/write-mbr.py and api::disk)
#       Passed to QEMU as a VirtIO block device (-drive file=disk_v3.img).
#       P1 @2048:   FAT32 interop volume (/mnt/sd)
#       P2 @526336: Cell bootstrap table read by the early loader.
#       P3 @560000: kernel heap snapshot region (Phase 29).
#       P4 @800000: littlefs /data volume (power-safe persistent store).
#       SpawnFromPath uses the P2 table to load VFS, config, shell.

$kernel_root = Get-Location
$tools_dir   = "$kernel_root/tools"
$rel_dir     = "$kernel_root/target/riscv64gc-unknown-none-elf/release"

# Linux runners ship `python3` only; Windows dev boxes ship `python`.
$python = if (Get-Command python -ErrorAction SilentlyContinue) { "python" } else { "python3" }

# Toolchain for the littlefs C core inside service-vfs (littlefs2-sys):
# cross-compile with the xpack riscv gcc; bindgen needs a 64-bit libclang
# (the VS BuildTools x64 copy). LFS_NO_INTRINSICS avoids __bswapsi2/__popcountdi2
# libcalls whose prebuilt compiler-builtins objects carry a soft-float ABI tag
# and refuse to link with our lp64d objects.
# Respect a pre-set CC so Linux CI can point at its distro toolchain
# (gcc-riscv64-unknown-elf → riscv64-unknown-elf-gcc) — same for OBJCOPY below.
if (-not $env:CC_riscv64gc_unknown_none_elf) {
    $env:CC_riscv64gc_unknown_none_elf = "riscv-none-elf-gcc"
}
if (-not $env:CFLAGS_riscv64gc_unknown_none_elf) {
    $env:CFLAGS_riscv64gc_unknown_none_elf = "-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS"
}
if (-not $env:LIBCLANG_PATH) {
    $vsLlvm = "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin"
    if (Test-Path "$vsLlvm/libclang.dll") { $env:LIBCLANG_PATH = $vsLlvm }
}

# 1. Build ALL cells in release mode.
# Release binaries are 10-100x smaller than debug, which matters because
# SpawnFromPath copies the full ELF into the 16MB kernel heap.
# Debug VFS=5.7MB, release VFS=3MB; Debug net=4.2MB, release net=~1MB.
Write-Host "Building release cells..."

# Fail-fast on core cell build errors: continuing would re-sign and ship the
# PREVIOUS binary from target/ — a silent build-skew that makes every later
# QEMU verify meaningless (you debug code that is not on the disk).
#
# CRITICAL: `cargo … | Select-Object` sets $LASTEXITCODE to Select-Object's exit
# (always 0), MASKING a cargo failure. So Build-Cargo captures output into a
# variable first — then $LASTEXITCODE reflects cargo — prints the tail, and only
# then checks. `mode 'core'` aborts; mode 'optional' records for the summary.
function Build-Cargo {
    param([string[]]$Packages, [string]$What, [string]$Mode = 'core', [int]$Tail = 4)
    $pkgArgs = @()
    foreach ($p in $Packages) { $pkgArgs += @('-p', $p) }
    $out = & cargo build --release @pkgArgs 2>&1
    $code = $LASTEXITCODE                    # capture BEFORE any pipe resets it
    $out | Select-Object -Last $Tail
    if ($code -ne 0) {
        if ($Mode -eq 'optional') {
            $script:FailedOptional += $What
        } else {
            Write-Host "FATAL: cargo build failed for $What — aborting before a stale binary is signed/shipped." -ForegroundColor Red
            exit 1
        }
    }
}
# Legacy shim: some later cargo lines still pipe to Select-Object and then call
# Assert-BuildOk. Those are the raw-pipe form and remain UNRELIABLE — migrate
# them to Build-Cargo when touched. Kept only so an untouched call still errors
# loudly on the obvious case (Select-Object rarely fails, so treat any non-zero
# as fatal for the paths that still use it).
function Assert-BuildOk([string]$what) {
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FATAL: cargo build failed for $what — aborting before a stale binary is signed/shipped." -ForegroundColor Red
        exit 1
    }
}
# Optional demo cells: never abort the disk, but record and report loudly at
# the end — a stale demo binary may be shipped in place of the failed build.
$script:FailedOptional = @()
function Add-FailedOptional([string]$what) {
    if ($LASTEXITCODE -ne 0) { $script:FailedOptional += $what }
}

Build-Cargo -What "core services + drivers" -Tail 5 -Packages @(
    'app-init', 'app-shell', 'service-platform',
    'service-vfs', 'service-config',
    'service-input', 'service-net', 'service-compositor', 'service-net-broker',
    'supervisor', 'driver-nvme', 'driver-e1000', 'driver-virtio-net', 'driver-virtio-blk', 'driver-virtio-gpu')
Build-Cargo -What "app-bench"      -Packages @('app-bench')       # builds bench + bench-probe
Build-Cargo -What "app-net-tools"  -Packages @('app-net-tools')
Build-Cargo -What "app-sys-tools"  -Packages @('app-sys-tools')
Build-Cargo -What "robot-demo + robot-dashboard" -Packages @('robot-demo', 'robot-dashboard')
Build-Cargo -What "fb-console"     -Packages @('fb-console')
Build-Cargo -What "hypha cells"    -Packages @('hypha-llm-gateway', 'hypha-core', 'hypha-tool-fs', 'hypha-tool-sys', 'hypha-tool-spawn')
Build-Cargo -What "input-test"     -Packages @('input-test')
Build-Cargo -What "audio-demo"     -Packages @('audio-demo')      # VirtIO sound test tone
Build-Cargo -What "app-https-demo" -Packages @('app-https-demo')  # G14 TLS server-auth e2e gate
Build-Cargo -What "app-http-smoke" -Packages @('app-http-smoke')  # ostd::http + ostd::json e2e gate
Build-Cargo -What "cfi-test" -Packages @('cfi-test')   # Layer-2 CFI violation test cell

# DOOM — only if doomgeneric sources have been cloned. Custom --target + -Z so
# it can't use Build-Cargo; capture the exit code BEFORE the pipe (see Build-Cargo).
$doom_src = "cells/demos/doom/src/c/doomgeneric/doomgeneric"
if (Test-Path $doom_src) {
    Write-Host "Building DOOM cell..."
    $doomOut = & cargo build --release -p doom --target riscv64gc-unknown-none-elf -Z build-std=core,alloc 2>&1
    $doomCode = $LASTEXITCODE
    $doomOut | Select-Object -Last 3
    if ($doomCode -ne 0) { $script:FailedOptional += "doom" }
} else {
    Write-Host "Skipping DOOM (clone doomgeneric to $doom_src first)."
}

# Tetris (pure Rust) — no external deps, always buildable.
Write-Host "Building Tetris (pure Rust)..."
Build-Cargo -What "tetris" -Mode optional -Tail 3 -Packages @('tetris')

# Tetris-C — needs Banaxi-Tech/Tetris-OS cloned into src/c/tetris-os/.
$tetris_os_src = "cells/demos/tetris-c/src/c/tetris-os"
if (Test-Path $tetris_os_src) {
    Write-Host "Building Tetris-C cell (Banaxi-Tech/Tetris-OS port)..."
    Build-Cargo -What "tetris-c" -Mode optional -Tail 3 -Packages @('tetris-c')
} else {
    Write-Host "Skipping Tetris-C (clone to $tetris_os_src first)."
}

# Tetris-Lua — embeds Lua 5.4 + tetris.lua via include_bytes!, shared C sources from lua runtime.
Write-Host "Building Tetris-Lua cell..."
Build-Cargo -What "tetris-lua" -Mode optional -Tail 3 -Packages @('tetris-lua')

# 1c. Build Zig cells (optional — requires zig 0.13+ in PATH).
$zig_elfs = @{}
$zig_output = & pwsh "$kernel_root/scripts/build-zig-cells.ps1" 2>&1
foreach ($line in $zig_output) {
    if ($line -match '^cell:(.+)=(.+)$') {
        $zig_elfs[$Matches[1]] = $Matches[2]
        Write-Host "  Zig cell ready: $($Matches[1]) -> $($Matches[2])"
    } else {
        Write-Host $line
    }
}

# ── Cell binary signing (Ed25519) ────────────────────────────────────────────
# Sign each cell ELF with the dev key before embedding. Runs here — inside
# gen_disk — so signing is never accidentally skipped (a separate wrapper could
# be bypassed; this cannot). The dev seed [0x43]*32 is fixed so rebuilds are
# reproducible and no key paste is required.
#
# sign-cell.py reads $env:OBJCOPY to select the correct cross-objcopy binary.
# Default to the xpack RISC-V toolchain; override before invoking this script.
if (-not $env:OBJCOPY) { $env:OBJCOPY = "riscv-none-elf-objcopy" }
Write-Host "Signing cell binaries (Ed25519 dev key, objcopy=$($env:OBJCOPY))..."
$sign_script = "scripts/sign-cell.py"
if (-not (Test-Path $sign_script)) {
    Write-Host "ERROR: $sign_script not found — run from the Cellos repo root." -ForegroundColor Red
    exit 1
}

function Invoke-SignCell {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return }  # optional cells handled below
    Write-Host "  signing $Path"
    & $python $sign_script --in $Path --out $Path
    if ($LASTEXITCODE -ne 0) {
        Write-Host "ERROR: sign-cell.py failed for $Path" -ForegroundColor Red
        exit 1
    }
}

# Sign the cells that are embedded / placed in the disk image.
Invoke-SignCell "$rel_dir/app-init"
Invoke-SignCell "$rel_dir/app-shell"
Invoke-SignCell "$rel_dir/platform"
Invoke-SignCell "$rel_dir/service-vfs"
Invoke-SignCell "$rel_dir/service-config"
Invoke-SignCell "$rel_dir/service-net"
Invoke-SignCell "$rel_dir/service-net-broker"
Invoke-SignCell "$rel_dir/service-compositor"
Invoke-SignCell "$rel_dir/supervisor"
Invoke-SignCell "$rel_dir/driver-nvme"
Invoke-SignCell "$rel_dir/driver-e1000"
Invoke-SignCell "$rel_dir/driver-virtio-net"
Invoke-SignCell "$rel_dir/driver-virtio-blk"
Invoke-SignCell "$rel_dir/driver-virtio-gpu"
Invoke-SignCell "$rel_dir/service-input"
Invoke-SignCell "$rel_dir/app-bench"
Invoke-SignCell "$rel_dir/bench-probe"
Invoke-SignCell "$rel_dir/app-net-tools"
Invoke-SignCell "$rel_dir/app-sys-tools"
Invoke-SignCell "$rel_dir/robot-demo"
Invoke-SignCell "$rel_dir/robot-dashboard"
Invoke-SignCell "$rel_dir/fb-console"
Invoke-SignCell "$rel_dir/hypha-llm-gateway"
Invoke-SignCell "$rel_dir/hypha-core"
Invoke-SignCell "$rel_dir/hypha-tool-fs"
Invoke-SignCell "$rel_dir/hypha-tool-sys"
Invoke-SignCell "$rel_dir/hypha-tool-spawn"
Invoke-SignCell "$rel_dir/input-test"
Invoke-SignCell "$rel_dir/audio-demo"
Invoke-SignCell "$rel_dir/app-https-demo"
Invoke-SignCell "$rel_dir/http-smoke"
Invoke-SignCell "$rel_dir/cfi-test"
Invoke-SignCell "$rel_dir/hotswap-demo-v1"
Invoke-SignCell "$rel_dir/hotswap-demo-v2"
Invoke-SignCell "$rel_dir/ls"
Invoke-SignCell "$rel_dir/cat"
Invoke-SignCell "$rel_dir/echo"
Invoke-SignCell "$rel_dir/ps"
Invoke-SignCell "$rel_dir/kill"
if (Test-Path "$rel_dir/lua")          { Invoke-SignCell "$rel_dir/lua" }
if (Test-Path "$rel_dir/doom")         { Invoke-SignCell "$rel_dir/doom" }
if (Test-Path "$rel_dir/tetris")       { Invoke-SignCell "$rel_dir/tetris" }
if (Test-Path "$rel_dir/tetris-c")     { Invoke-SignCell "$rel_dir/tetris-c" }
if (Test-Path "$rel_dir/tetris-lua")   { Invoke-SignCell "$rel_dir/tetris-lua" }
if (Test-Path "$rel_dir/micropython")  { Invoke-SignCell "$rel_dir/micropython" }
if (Test-Path "$rel_dir/posix-shim-test") { Invoke-SignCell "$rel_dir/posix-shim-test" }
# Sign Zig cells
foreach ($zig_path in $zig_elfs.Values) {
    if (Test-Path $zig_path) { Invoke-SignCell $zig_path }
}

Write-Host "All cells signed."

# 1b. Update kernel embedded cells (init, shell, vfs, config) from release builds.
# These 4 cells are embedded in kernel_fs.img via include_bytes!.
# NOTE: cells are already signed in-place by Sign-Cell above.
Write-Host "Updating kernel embedded cells..."
$embedded = "kernel/src/embedded"
# Only `init` is embedded as a separate blob (kernel/src/main.rs INIT_ELF).
# shell/vfs/config/lua ship inside kernel_fs.img — the old standalone copies
# were dead weight that churned git on every gen_disk run.
Copy-Item "$rel_dir/app-init"       "$embedded/init"   -Force

# 2. Paths — all bootstrap table entries use RELEASE builds.
$init_bin   = "$rel_dir/app-init"
$shell_bin  = "$rel_dir/app-shell"
$vfs_bin    = "$rel_dir/service-vfs"
$config_bin = "$rel_dir/service-config"
$lua_bin    = "$rel_dir/lua"
$doom_bin   = "$rel_dir/doom"              # DOOM cell (needs doomgeneric clone first)
$doom_wad   = "doom1.wad"                  # shareware WAD — place at d:/ViCell/doom1.wad
$tetris_bin     = "$rel_dir/tetris"        # Tetris — pure Rust, no external deps
$tetris_c_bin   = "$rel_dir/tetris-c"     # Tetris-C — Banaxi-Tech/Tetris-OS port
$tetris_lua_bin = "$rel_dir/tetris-lua"   # Tetris-Lua — Lua 5.4 embedded, tetris.lua included
$upy_bin    = "$rel_dir/micropython"       # Phase 18: MicroPython runtime cell
$bench_bin       = "$rel_dir/bench"             # Phase 22 benchmark cell
$bench_probe_bin = "$rel_dir/bench-probe"      # bench probe/load child (VA 0x19000000)
$input_bin  = "$rel_dir/service-input"     # Phase 14: input service cell
$net_bin    = "$rel_dir/service-net"       # Phase 15: network service cell
$net_broker_bin   = "$rel_dir/service-net-broker" # L.0: cluster net-broker cell
$supervisor_bin   = "$rel_dir/supervisor"         # Kernel Boundary Law: hotswap orchestration
$platform_bin     = "$rel_dir/platform"            # Kernel Boundary Law: PCIe ECAM Platform Cell
$nvme_bin         = "$rel_dir/driver-nvme"        # Kernel Boundary Law: NVMe PCIe Driver Cell
$e1000_bin        = "$rel_dir/driver-e1000"       # Kernel Boundary Law: e1000 PCIe Driver Cell
$virtio_net_bin   = "$rel_dir/driver-virtio-net"  # Kernel Boundary Law: VirtIO MMIO NIC Driver Cell
$virtio_blk_bin   = "$rel_dir/driver-virtio-blk"  # G2 loader redesign: VirtIO MMIO Block Driver Cell
$virtio_gpu_bin   = "$rel_dir/driver-virtio-gpu"  # Kernel Boundary Law: VirtIO GPU Driver Cell
$comp_bin      = "$rel_dir/service-compositor" # Phase 16: compositor + GPU
$fb_console_bin = "$rel_dir/fb-console"       # HMI: mirror kernel log to HDMI screen
$robot_demo_bin = "$rel_dir/robot-demo"       # G1 sensor→actuator reference demo
$dashboard_bin = "$rel_dir/robot-dashboard"  # G1 ViUI v2 dashboard demo
$hypha_llm_bin = "$rel_dir/hypha-llm-gateway" # Hypha P0 — LLM network gateway
$hypha_core_bin = "$rel_dir/hypha-core"       # Hypha P1 — agent brain (chat)
$hypha_tool_fs_bin    = "$rel_dir/hypha-tool-fs"    # Hypha P2 — filesystem tool cell
$hypha_tool_sys_bin   = "$rel_dir/hypha-tool-sys"   # Hypha P3 — system introspection tool cell
$hypha_tool_spawn_bin = "$rel_dir/hypha-tool-spawn" # Hypha P3 — cell lifecycle tool cell
$nc_bin     = "$rel_dir/nc"               # Phase A: TCP netcat tool
$curl_bin   = "$rel_dir/curl"             # Phase B: HTTP GET client
$wget_bin   = "$rel_dir/wget"             # Phase U: HTTP wget tool
$httpd_bin  = "$rel_dir/httpd"            # Phase U: HTTP server
$mqtt_bin   = "$rel_dir/mqtt"             # Phase X-5: MQTT client
$posix_shim_test_bin = "$rel_dir/posix-shim-test"  # Tier 1b POSIX shim test cell
$input_test_bin      = "$rel_dir/input-test"       # P05 bare-cell input delivery test
# Zig cells — paths resolved by build-zig-cells.ps1 into $zig_elfs hashtable
$audio_bin = "$rel_dir/audio-demo"   # VirtIO sound test-tone cell (shell: `audio-demo`)
$https_demo_bin = "$rel_dir/app-https-demo"  # G14 TLS server-auth e2e gate (shell: `https-demo`)
$http_smoke_bin = "$rel_dir/http-smoke"      # ostd::http + ostd::json e2e gate (shell: `http-smoke`)
$cfi_test_bin   = "$rel_dir/cfi-test"        # Layer-2 CFI violation test (shell: `cfi-test`)
$hotswap_demo_v1_bin = "$rel_dir/hotswap-demo-v1"  # M4.1 hotswap demo cell v1
$hotswap_demo_v2_bin = "$rel_dir/hotswap-demo-v2"  # M4.1 hotswap demo cell v2
$ls_bin   = "$rel_dir/ls"    # M3.2 embedded debug utils
$cat_bin  = "$rel_dir/cat"
$echo_bin = "$rel_dir/echo"
$ps_bin   = "$rel_dir/ps"
$kill_bin = "$rel_dir/kill"

foreach ($pair in @(
    @{ Path = $init_bin;   Name = "app-init" },
    @{ Path = $shell_bin;  Name = "app-shell" },
    @{ Path = $vfs_bin;    Name = "service-vfs" },
    @{ Path = $config_bin; Name = "service-config" }
)) {
    if (-not (Test-Path $pair.Path)) {
        Write-Host "Error: $($pair.Name) not found at $($pair.Path)"
        exit 1
    }
}

if (-not (Test-Path $lua_bin)) {
    Write-Host "Warning: Lua binary not found - skipping Lua in FAT32 image."
    $lua_bin = $null
}

if (-not (Test-Path $doom_bin)) {
    Write-Host "Warning: DOOM binary not found - skipping DOOM in FAT32 image."
    $doom_bin = $null
}
if (-not (Test-Path $doom_wad)) {
    Write-Host "Warning: doom1.wad not found at $doom_wad - skipping WAD in FAT32 image."
    $doom_wad = $null
}

if (-not (Test-Path $upy_bin)) {
    Write-Host "Warning: MicroPython binary not found - skipping python in FAT32 image."
    $upy_bin = $null
}

if (-not (Test-Path $bench_bin)) {
    Write-Host "Warning: bench binary not found - run 'cargo build -p app-bench' first."
    $bench_bin = $null
}

# 3a. Generate kernel_fs.img (small embedded FAT32, ~8 MB, with release cells).
#     This image is embedded in the kernel binary via ramdisk.rs.
Write-Host "Generating kernel_fs.img (embedded FAT32, release cells)..."
$tmpDir = "$env:TEMP/ViCell_kfs"
New-Item -ItemType Directory -Force $tmpDir | Out-Null
Set-Content -Path "$tmpDir/hostname" -Value "ViCell" -NoNewline -Encoding ascii
Set-Content -Path "$tmpDir/readme"   -Value "Welcome to ViCell!" -NoNewline -Encoding ascii
# G2 kernel-shrink: VIFS1 carries ONLY what must resolve before/without VFS.
#   1. Bootstrap cells (loader::early::BOOTSTRAP_CELLS + init): the chain that
#      brings up the Block Cell + VFS with no block driver in the kernel.
#   2. Kernel-FD-only data: /doom1.wad — DOOM reads it through mlibc → kernel
#      Open/Read syscalls, which resolve against VIFS1 only (kernel/src/fs.rs).
#      Moving it out needs mlibc file I/O → VFS routing (tracked in TODO).
#   3. hotswap-demo-v1/v2: kernel-side hotswap (cell/hotswap.rs) loads the new
#      ELF via loader::spawn_from_path (VIFS1/P2 only — no VFS from kernel).
# EVERYTHING else lives in the disk cell-store (table_args → P2 table + P6 FAT)
# and is spawned via VFS + sys_spawn_from_elf (ostd sys_spawn_from_path does
# this automatically once service::VFS is registered).
$kfs_args = @(
    "kernel/src/embedded/kernel_fs.img",
    "$rel_dir/app-init",       "/bin/init",
    "$rel_dir/app-shell",      "/bin/shell",
    "$rel_dir/service-vfs",    "/bin/vfs",
    "$rel_dir/service-config", "/bin/config",
    "$tmpDir/hostname",        "/etc/hostname",
    "$tmpDir/readme",          "/readme.txt"
)
if (Test-Path $platform_bin)   { $kfs_args += @($platform_bin,   "/bin/platform") }
if (Test-Path $virtio_blk_bin) { $kfs_args += @($virtio_blk_bin, "/bin/block") }
if ($doom_wad)  { $kfs_args += @($doom_wad, "/doom1.wad") }
if (Test-Path $hotswap_demo_v1_bin) { $kfs_args += @($hotswap_demo_v1_bin, "/bin/hotswap-demo-v1") }
if (Test-Path $hotswap_demo_v2_bin) { $kfs_args += @($hotswap_demo_v2_bin, "/bin/hotswap-demo-v2") }
# bench + bench-probe: same kernel-spawn-bound class as the hotswap demos —
# bench re-spawns itself/its probe via sys_spawn_pinned, which resolves through
# the KERNEL loader (VIFS1/P2 only, no VFS), so the child spawns need VIFS1.
if ($bench_bin)                       { $kfs_args += @($bench_bin,       "/bin/bench") }
if (Test-Path "$rel_dir/bench-probe") { $kfs_args += @($bench_probe_bin, "/bin/bench-probe") }
& $python "$tools_dir/mkfat32.py" @kfs_args 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Host "FATAL: mkfat32.py failed — kernel_fs.img is invalid." -ForegroundColor Red
    exit 1
}
Remove-Item -Recurse -Force $tmpDir
$kfs_mb = [Math]::Round((Get-Item "kernel/src/embedded/kernel_fs.img").Length/1MB,1)
Write-Host "  kernel_fs.img: ${kfs_mb} MB"

# 3b. Rebuild the kernel binary (embeds the new kernel_fs.img via include_bytes!).
#     Must be done before creating disk_v3.img so the test runner picks up the latest kernel.
Write-Host "Rebuilding kernel (embedding updated kernel_fs.img)..."
$env:RUSTFLAGS = "-C relocation-model=pic"
$kernOut = & cargo build --release -p vicell-kernel `
    --target riscv64gc-unknown-none-elf `
    -Z build-std=core,alloc 2>&1
$kernCode = $LASTEXITCODE                     # capture BEFORE the pipe (see Build-Cargo)
$kernOut | Select-Object -Last 3
Remove-Item Env:/RUSTFLAGS
if ($kernCode -ne 0) {
    Write-Host "FATAL: kernel rebuild failed — disk would ship a stale kernel with an old kernel_fs.img." -ForegroundColor Red
    exit 1
}

# 3c. Create a blank disk image for VirtIO block — MBR layout (Milestone 2.5 P03).
#     P1 FAT32 @2048+524288 · P2 cell-table @526336 · P3 snapshot @560000 · P4 littlefs @800000
#     Must match tools/write-mbr.py and kernel/src/loader/disk_layout.rs.
Write-Host "Creating blank disk image (disk_v3.img, MBR, ~577 MB)..."
# Grown for the P6 FAT cell-store (G2 loader redesign): base LBA 1_062_144 +
# 65_536 sectors = 1_127_680. Written non-sparsely below, so the array is large
# but transient. P1-P4 in the MBR; P5/P6 are constant-addressed (see api::disk).
$disk_sectors = 1127680
$diskSize = $disk_sectors * 512
$blankImg = New-Object byte[] $diskSize
[System.IO.File]::WriteAllBytes("disk_v3.img", $blankImg)
Write-Host "  Blank image created ($disk_sectors sectors)."

# 3c. Write the MBR partition table at LBA 0.
& $python "$tools_dir/write-mbr.py" "disk_v3.img" 2>&1
if ($LASTEXITCODE -ne 0) { throw "MBR write failed" }

# 3d. Format an empty FAT32 filesystem inside P1 (base LBA 2048).
#     65525+ data clusters at 8 sec/clus satisfy the FAT32 minimum.
Write-Host "Formatting FAT32 partition P1 (LBA 2048 + 524288 sectors)..."
& $python "$tools_dir/mkfat32_inplace.py" "disk_v3.img" 524288 2048 2>&1
if ($LASTEXITCODE -ne 0) { throw "FAT32 format failed - disk_v3.img may be corrupt" }

# 4. Append cell bootstrap table (for kernel early loader).
# Only include the cells that the kernel early loader needs: VFS, config, shell.
# Optionally include lua and bench when built.
Write-Host "Appending cell bootstrap table..."
$table_args = @(
    "disk_v3.img",
    "/bin/vfs=$vfs_bin",
    "/bin/config=$config_bin",
    "/bin/shell=$shell_bin"
)
if ($lua_bin)   { $table_args += "/bin/lua=$lua_bin" }
if ($upy_bin)   { $table_args += "/bin/python=$upy_bin" }
# Games moved off VIFS1 (G2 kernel-shrink) — spawn from the cell-store via VFS.
# DOOM's WAD stays in VIFS1 (/doom1.wad — kernel-FD read path); only the binary
# moves here.
if ($doom_bin)  { $table_args += "/bin/doom=$doom_bin" }
if (Test-Path $tetris_bin)     { $table_args += "/bin/tetris=$tetris_bin" }
if (Test-Path $tetris_c_bin)   { $table_args += "/bin/tetris-c=$tetris_c_bin" }
if (Test-Path $tetris_lua_bin) { $table_args += "/bin/tetris-lua=$tetris_lua_bin" }
if ($bench_bin)       { $table_args += "/bin/bench=$bench_bin" }
if (Test-Path "$rel_dir/bench-probe") { $table_args += "/bin/bench-probe=$bench_probe_bin" }
if (Test-Path $input_bin) { $table_args += "/bin/input=$input_bin" }
if (Test-Path $net_bin)   { $table_args += "/bin/net=$net_bin" }
if (Test-Path $net_broker_bin) { $table_args += "/bin/net-broker=$net_broker_bin" }
if (Test-Path $supervisor_bin) { $table_args += "/bin/supervisor=$supervisor_bin" }
if (Test-Path $platform_bin)   { $table_args += "/bin/platform=$platform_bin" }
if (Test-Path $nvme_bin)       { $table_args += "/bin/nvme=$nvme_bin" }
if (Test-Path $e1000_bin)      { $table_args += "/bin/e1000=$e1000_bin" }
if (Test-Path $virtio_net_bin) { $table_args += "/bin/virtio-net=$virtio_net_bin" }
if (Test-Path $virtio_blk_bin) { $table_args += "/bin/block=$virtio_blk_bin" }
if (Test-Path $virtio_gpu_bin) { $table_args += "/bin/virtio-gpu=$virtio_gpu_bin" }
if (Test-Path $comp_bin)        { $table_args += "/bin/compositor=$comp_bin" }
if (Test-Path $fb_console_bin)  { $table_args += "/bin/fb-console=$fb_console_bin" }
if (Test-Path $robot_demo_bin)  { $table_args += "/bin/robot-demo=$robot_demo_bin" }
if (Test-Path $dashboard_bin)   { $table_args += "/bin/robot-dashboard=$dashboard_bin" }
if (Test-Path $hypha_llm_bin)      { $table_args += "/bin/llm-gateway=$hypha_llm_bin" }
if (Test-Path $hypha_core_bin)     { $table_args += "/bin/hypha=$hypha_core_bin" }
if (Test-Path $hypha_tool_fs_bin)    { $table_args += "/bin/tool-fs=$hypha_tool_fs_bin" }
if (Test-Path $hypha_tool_sys_bin)   { $table_args += "/bin/tool-sys=$hypha_tool_sys_bin" }
if (Test-Path $hypha_tool_spawn_bin) { $table_args += "/bin/tool-spawn=$hypha_tool_spawn_bin" }
if (Test-Path $nc_bin)    { $table_args += "/bin/nc=$nc_bin" }
if (Test-Path $curl_bin)  { $table_args += "/bin/curl=$curl_bin" }
if (Test-Path $wget_bin)  { $table_args += "/bin/wget=$wget_bin" }
if (Test-Path $httpd_bin) { $table_args += "/bin/httpd=$httpd_bin" }
if (Test-Path $mqtt_bin)  { $table_args += "/bin/mqtt=$mqtt_bin" }
if (Test-Path $posix_shim_test_bin) { $table_args += "/bin/posix-shim-test=$posix_shim_test_bin" }
if (Test-Path $input_test_bin)      { $table_args += "/bin/input-test=$input_test_bin" }
if (Test-Path $audio_bin) { $table_args += "/bin/audio-demo=$audio_bin" }
if (Test-Path $https_demo_bin) { $table_args += "/bin/https-demo=$https_demo_bin" }
if (Test-Path $http_smoke_bin) { $table_args += "/bin/http-smoke=$http_smoke_bin" }
if (Test-Path $cfi_test_bin)   { $table_args += "/bin/cfi-test=$cfi_test_bin" }
if (Test-Path $hotswap_demo_v1_bin) { $table_args += "/bin/hotswap-demo-v1=$hotswap_demo_v1_bin" }
if (Test-Path $hotswap_demo_v2_bin) { $table_args += "/bin/hotswap-demo-v2=$hotswap_demo_v2_bin" }
# Zig cells (Tier 1b) — added when zig is in PATH and build-zig-cells.ps1 succeeds
foreach ($kv in $zig_elfs.GetEnumerator()) {
    $table_args += "/bin/$($kv.Key)=$($kv.Value)"
}
if (Test-Path $ls_bin)   { $table_args += "/bin/ls=$ls_bin" }
if (Test-Path $cat_bin)  { $table_args += "/bin/cat=$cat_bin" }
if (Test-Path $echo_bin) { $table_args += "/bin/echo=$echo_bin" }
if (Test-Path $ps_bin)   { $table_args += "/bin/ps=$ps_bin" }
if (Test-Path $kill_bin) { $table_args += "/bin/kill=$kill_bin" }
& $python "$tools_dir/write-cell-table.py" @table_args
if ($LASTEXITCODE -ne 0) {
    Write-Host "FATAL: write-cell-table.py failed — disk_v3.img bootstrap table is invalid." -ForegroundColor Red
    exit 1
}

# ── P6: FAT cell-store (G2 loader redesign) ───────────────────────────────────
# Build a standalone FAT16 volume holding every cell ELF at the FAT ROOT (by
# basename), then write it into disk_v3.img at PART_CELLSTORE_BASE_LBA. The VFS
# `/bin` BinOverlay reads this after a VIFS1 miss, so non-bootstrap cells stay
# reachable once the raw P2 table + kernel block reader are retired (phases
# 05-06). Reuses $table_args (source of truth for the cell set) — VIFS1 wins in
# the overlay for cells present in both, so the superset is harmless.
$CELLSTORE_BASE_LBA = 1062144   # MUST match api::disk::PART_CELLSTORE_BASE_LBA
$CELLSTORE_SECTORS  = 65536     # MUST match api::disk::PART_CELLSTORE_SECTORS (32 MB)
Write-Host "Building FAT cell-store (P6 @ LBA $CELLSTORE_BASE_LBA)..."
$cellstore_args = @("cell_store.img")
foreach ($entry in $table_args) {
    if ($entry -notmatch '=') { continue }      # skip the leading "disk_v3.img" target arg
    $kv       = $entry -split '=', 2            # "/bin/<name>=<srcpath>"
    $basename = $kv[0] -replace '^/bin/', ''    # "<name>" — FAT root, matches FatBackend("/bin") strip
    $cellstore_args += @($kv[1], "/$basename")
}
& $python "$tools_dir/mkfat32.py" @cellstore_args
if ($LASTEXITCODE -ne 0) {
    Write-Host "FATAL: mkfat32.py failed — cell_store.img is invalid." -ForegroundColor Red
    exit 1
}
$storeBytes = [System.IO.File]::ReadAllBytes("cell_store.img")
if ($storeBytes.Length -gt ($CELLSTORE_SECTORS * 512)) {
    Write-Host "FATAL: cell_store.img ($([Math]::Round($storeBytes.Length/1MB,1)) MB) exceeds the $([int]($CELLSTORE_SECTORS/2048)) MB P6 window — grow PART_CELLSTORE_SECTORS + \$disk_sectors." -ForegroundColor Red
    exit 1
}
$dfs = [System.IO.File]::Open((Resolve-Path "disk_v3.img"), 'Open', 'Write')
$dfs.Seek([long]$CELLSTORE_BASE_LBA * 512, 'Begin') | Out-Null
$dfs.Write($storeBytes, 0, $storeBytes.Length)
$dfs.Close()
Remove-Item -Force "cell_store.img"
Write-Host "  cell-store written: $([Math]::Round($storeBytes.Length/1MB,1)) MB at LBA $CELLSTORE_BASE_LBA"

if ($script:FailedOptional.Count -gt 0) {
    Write-Host ""
    Write-Host "⚠ WARNING: $($script:FailedOptional.Count) optional cell(s) FAILED to build: $($script:FailedOptional -join ', ')" -ForegroundColor Yellow
    Write-Host "  The disk may carry a STALE previous binary for each — do not debug those cells on this image." -ForegroundColor Yellow
}
Write-Host "Done. disk_v3.img is ready."
Write-Host ""
Write-Host "To run benchmarks: boot QEMU and run '/bin/bench' from the shell."

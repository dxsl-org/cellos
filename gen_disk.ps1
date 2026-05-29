# Generate Disk Image — FAT32 primary filesystem + cell bootstrap table.
#
# Layout of disk_v3.img:
#   LBA       0 – 81 999 : FAT32 filesystem (~42 MB), served by the VFS Cell.
#   LBA  82 000 +        : Cell bootstrap table (header + entries + raw ELFs),
#                          read by the kernel early loader before VFS is up.
#
# The bootstrap table lets init spawn VFS, config, and shell via SpawnFromPath
# without embedding those ELFs in the init binary.

$kernel_root = Get-Location
$tools_dir   = "$kernel_root\tools"
$target_dir  = "$kernel_root\target\riscv64gc-unknown-none-elf\debug"
$rel_dir     = "$kernel_root\target\riscv64gc-unknown-none-elf\release"

# 1. Build all cells
Write-Host "Building cells..."
cargo build -p app-init
cargo build -p app-shell
cargo build -p service-vfs

cargo build -p service-config
cargo build -p app-bench        # Phase 22: benchmarking cell
cargo build -p app-utils        # Phase 17b: standard utilities (wc, head, tail, grep, sort, sed, …)
cargo build -p app-sys-tools    # Phase 17b: system tools (ps, uname, date, free, env, shutdown)
cargo build -p app-net-tools    # Phase 17b: network tools (ping, curl, nc, wget — stubs)

# 2. Paths
$init_bin   = "$target_dir\app-init"
$shell_bin  = "$target_dir\app-shell"
$vfs_bin    = "$target_dir\service-vfs"
$config_bin = "$target_dir\service-config"
$lua_bin    = "$rel_dir\lua"
$bench_bin  = "$target_dir\bench"       # Phase 22 benchmark cell

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
    Write-Host "Warning: Lua binary not found — skipping Lua in FAT32 image."
    $lua_bin = $null
}

if (-not (Test-Path $bench_bin)) {
    Write-Host "Warning: bench binary not found — run 'cargo build -p app-bench' first."
    $bench_bin = $null
}

# 3. Create a blank disk image (40MB = 81920 sectors).
# The FAT32 region is reserved for future use by the VFS Cell once it can
# access VirtIO block sectors directly.  For now, SpawnFromPath reads cells
# exclusively from the bootstrap table in step 4.
Write-Host "Creating blank disk image (disk_v3.img)..."
$diskSize = 81920 * 512     # 40 MB — matches CELL_TABLE_BASE_LBA = 82000
$blankImg = New-Object byte[] $diskSize
[System.IO.File]::WriteAllBytes("disk_v3.img", $blankImg)
Write-Host "  Blank 40 MB image created."

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
if ($bench_bin) { $table_args += "/bin/bench=$bench_bin" }
python "$tools_dir\write-cell-table.py" @table_args

Write-Host "Done. disk_v3.img is ready."
Write-Host ""
Write-Host "To run benchmarks: boot QEMU and run '/bin/bench' from the shell."

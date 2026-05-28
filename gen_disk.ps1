# Generate Disk Image
$kernel_root = Get-Location
$tools_dir = "$kernel_root\tools"
$target_dir = "$kernel_root\target\riscv64gc-unknown-none-elf\debug"
$cells_dir = "$kernel_root\cells\apps"

# 1. Build Apps
Write-Host "Building Apps..."
cargo build -p app-init
cargo build -p app-shell

# 2. Check Paths
$init_bin = "$target_dir\app-init"
$shell_bin = "$target_dir\app-shell"
$lua_bin = "$kernel_root\target\riscv64gc-unknown-none-elf\release\lua"
$mpy_bin = "$kernel_root\target\riscv64gc-unknown-none-elf\release\micropython"

if (-not (Test-Path $init_bin)) {
    Write-Host "Error: Init binary not found at $init_bin"
    exit 1
}

if (-not (Test-Path $shell_bin)) {
    Write-Host "Error: Shell binary not found at $shell_bin"
    exit 1
}

if (-not (Test-Path $lua_bin)) {
    Write-Host "Error: Lua binary not found at $lua_bin"
    exit 1
}

if (-not (Test-Path $mpy_bin)) {
    Write-Host "Warning: MicroPython binary not found at $mpy_bin. Skipping."
    # Remove mpy from args if missing
    python "$tools_dir\mkfat32.py" "disk_v3.img" $init_bin "init" $shell_bin "shell" $lua_bin "lua"
} else {
    Write-Host "Generating disk_v3.img (40MB) with MicroPython..."
    python "$tools_dir\mkfat32.py" "disk_v3.img" $init_bin "init" $shell_bin "shell" $lua_bin "lua" $mpy_bin "micropython"
}

Write-Host "Done."

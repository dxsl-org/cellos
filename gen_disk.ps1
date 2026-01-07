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

if (-not (Test-Path $init_bin)) {
    Write-Host "Error: Init binary not found at $init_bin"
    exit 1
}

if (-not (Test-Path $shell_bin)) {
    Write-Host "Error: Shell binary not found at $shell_bin"
    exit 1
}

# 3. Create Disk Image
Write-Host "Generating disk.img (512MB)..."
python "$tools_dir\mkfat32.py" "disk.img" $init_bin "init" $shell_bin "shell"

Write-Host "Done."

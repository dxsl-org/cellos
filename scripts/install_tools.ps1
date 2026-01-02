Write-Host "Installing Cargo Tools for ViOS Development..."

$tools = @(
    "cargo-bloat",
    "cargo-asm",
    "cargo-modules",
    "cargo-audit",
    "cargo-expand",
    "cargo-fuzz",
    "cargo-binutils"
)

foreach ($tool in $tools) {
    Write-Host "Checking $tool..."
    if (Get-Command $tool -ErrorAction SilentlyContinue) {
        Write-Host "  $tool is already installed."
    } else {
        Write-Host "  Installing $tool..."
        cargo install $tool
    }
}

Write-Host "Setup Complete."

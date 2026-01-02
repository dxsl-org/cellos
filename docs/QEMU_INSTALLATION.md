# 🔧 Hướng Dẫn Cài Đặt QEMU cho Windows

## Phương Pháp 1: Tải Trực Tiếp (Khuyến Nghị)

### Bước 1: Tải QEMU
1. Mở trình duyệt và truy cập: https://qemu.weilnetz.de/w64/
2. Tìm phiên bản mới nhất (hiện tại là **QEMU 10.2.0**)
3. Tải file: `qemu-w64-setup-20251224.exe` (hoặc phiên bản mới nhất)

### Bước 2: Cài Đặt
1. Chạy file installer vừa tải
2. Chọn "Next" và giữ nguyên các thiết lập mặc định
3. Đường dẫn cài đặt mặc định: `C:\Program Files\qemu`
4. Hoàn tất cài đặt

### Bước 3: Thêm QEMU vào PATH
Mở PowerShell **với quyền Administrator** và chạy:

```powershell
# Thêm QEMU vào PATH
$qemuPath = "C:\Program Files\qemu"
$currentPath = [Environment]::GetEnvironmentVariable("Path", "Machine")
if ($currentPath -notlike "*$qemuPath*") {
    [Environment]::SetEnvironmentVariable(
        "Path",
        "$currentPath;$qemuPath",
        "Machine"
    )
    Write-Host "✅ QEMU đã được thêm vào PATH!" -ForegroundColor Green
} else {
    Write-Host "✅ QEMU đã có trong PATH!" -ForegroundColor Green
}
```

### Bước 4: Khởi Động Lại Terminal
Đóng và mở lại PowerShell để cập nhật PATH.

### Bước 5: Kiểm Tra
```powershell
qemu-system-riscv64 --version
```

Bạn sẽ thấy output như:
```
QEMU emulator version 10.2.0
Copyright (c) 2003-2025 Fabrice Bellard and the QEMU Project developers
```

---

## Phương Pháp 2: Cài Đặt Tự Động (Nếu Có Quyền Admin)

Chạy script PowerShell sau **với quyền Administrator**:

```powershell
# Download QEMU installer
$url = "https://qemu.weilnetz.de/w64/2024/qemu-w64-setup-20241224.exe"
$installer = "$env:TEMP\qemu-installer.exe"

Write-Host "📥 Downloading QEMU..." -ForegroundColor Yellow
Invoke-WebRequest -Uri $url -OutFile $installer

Write-Host "🚀 Installing QEMU..." -ForegroundColor Yellow
Start-Process -FilePath $installer -ArgumentList "/S" -Wait

Write-Host "✅ Installation complete!" -ForegroundColor Green
```

---

## ⚠️ Lưu Ý

- QEMU yêu cầu **Windows 8 trở lên**
- Nếu gặp lỗi "không tìm thấy lệnh", hãy:
  1. Kiểm tra QEMU đã được cài đặt tại `C:\Program Files\qemu`
  2. Đảm bảo PATH đã được cập nhật
  3. Khởi động lại terminal

---

## 🚀 Bước Tiếp Theo

Sau khi cài đặt QEMU thành công, bạn có thể:

1. **Build ViOS kernel**:
   ```powershell
   cargo build --no-default-features -p kernel
   ```

2. **Chạy ViOS trên QEMU**:
   ```powershell
   .\scripts\run_qemu.ps1
   ```

---

## 🆘 Troubleshooting

### Lỗi: "qemu-system-riscv64 is not recognized"
- **Nguyên nhân**: QEMU chưa được thêm vào PATH
- **Giải pháp**: Chạy lại Bước 3 ở trên

### Lỗi: "Access denied"
- **Nguyên nhân**: Thiếu quyền Administrator
- **Giải pháp**: Chạy PowerShell với "Run as Administrator"

### QEMU chạy nhưng kernel không boot
- **Nguyên nhân**: Kernel chưa được build
- **Giải pháp**: Chạy `cargo build --no-default-features -p kernel`

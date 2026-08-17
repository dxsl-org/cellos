@echo off
cd /d D:\ViCell
cargo check -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc > D:\ViCell\kernel_check.txt 2>&1
echo EXIT:%ERRORLEVEL% >> D:\ViCell\kernel_check.txt

## Cấp độ 1: Giả lập tập lệnh bằng Phần mềm (ISA Simulator)
- Sửa Trình biên dịch (Compiler Toolchain): Mày tải bộ LLVM hoặc GCC RISC-V về. Mày phải "dạy" cho cái compiler biết mã Opcode (mã nhị phân) của lệnh GRANT.CELL hay SETDEADLINE là gì.
- Dùng Spike hoặc QEMU: Đây là các trình giả lập tập lệnh (ISA Simulator) phổ biến nhất. Mày viết logic của lệnh mới bằng C/C++ và nhúng vào QEMU. (Ví dụ: Dạy QEMU rằng khi gặp opcode Xcell, hãy kiểm tra 12-bit tag).
- Boot Cellos: Biên dịch Cellos OS bằng cái compiler vừa độ, ném file nhị phân vào QEMU chạy thử. Nếu Supervisor bắt được lỗi đúng như lý thuyết thì logic kiến trúc pass.

## Cấp độ 2: Mô phỏng mạch phần cứng (RTL Simulation)
Logic đúng chưa chắc phần cứng chạy được. Phải viết mã RTL (Register Transfer Level) để mô tả các cổng logic.
- Chọn lõi nguồn mở: Đừng tự viết từ số 0. Hãy lấy lõi VexRiscv (viết bằng SpinalHDL) làm nền tảng.
- Nhét lệnh vào lõi: Mày chọc vào phần ALU (bộ tính toán) và Decoder (bộ giải mã lệnh) của lõi đó, móc thêm các cổng logic để xử lý lệnh Xgrant hoặc Xprobe của mày.
- Chạy Verilator: Đây là công cụ mô phỏng thần thánh. Nó sẽ dịch đống mã RTL (SpinalHDL) của mày thành mã C++ và chạy chu kỳ xung nhịp (cycle-accurate). Mày dựng một container Docker chứa Verilator ngay trên WSL2, ném file RTL vào, nó sẽ chạy và xuất ra file sóng (waveform .vcd). Mày mở file này bằng GTKWave để soi từng bit nháy lên nháy xuống xem mạch có bị kẹt hay trễ chu kỳ không.

## Cấp độ 3: Ném lên board FPGA (Hardware Prototyping)
Mô phỏng Verilator cực kỳ chính xác nhưng chạy rất chậm (chỉ vài ngàn lệnh/giây). Để chạy mượt hệ điều hành và giao tiếp với thế giới thực, mày cần FPGA.
- Đổ code vào FPGA: Dùng các board như Xilinx ZCU102 (hoặc một con FPGA tầm trung vừa túi tiền). Mày biên dịch bộ RTL vừa test ở bước 2 thành bitstream và nạp vào FPGA.
- Chạy Benchmark: Lúc này con FPGA biến thành con chip RISC-V thực thụ chạy ở tốc độ vài chục MHz. Mày boot Cellos lên, đo đạc tốc độ IPC, độ trễ Context Switch xem có đạt mức ~10ns như kỳ vọng không.
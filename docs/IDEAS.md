# Ideas

## Must have
- Thiết kế một Network Fabric (Mạng kết nối phần cứng) riêng (giống cách Google làm cáp quang nội bộ cho TPU).
- Mở rộng Xgrant để nó có thể "chuyển nhượng quyền sở hữu vùng nhớ" xuyên suốt các node vật lý khác nhau thông qua mạng quang học siêu tốc. Zero-copy IPC across boards.
- Distributed SAS (Không gian địa chỉ phân tán): có thể áp dụng nguyên lý thiết kế đệ quy phân lớp để các cụm node giao tiếp địa chỉ bộ nhớ với nhau một cách xuyên suốt mà không dẫm chân lên nhau.
- Thiết kế lõi theo dạng Chiplet UCIe: Lõi điều khiển (Controller Die) và Lõi tính toán (Compute Die) lắp trên cùng một đế silicon
- Suy nghĩ thêm về 1 tập lệnh bảo mật/mã hóa chuyên dụng cho chip Cellos.


## Reference
- rust-raspberrypi-OS-tutorials — ground truth cho EL2→EL1, GIC v2, PL011 (đối chiếu hal/arch/arm/)
- awesome-embedded-rust — discovery tool tìm sensor driver crates tương thích embedded-hal

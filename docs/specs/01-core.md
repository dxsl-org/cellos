# Cellos Architecture: Core System
**Version**: 0.3 (Cellular SAS - Enhanced Integrity)
**Status**: Definitive

---

## 1. System Philosophy
Cellos dịch chuyển từ cách ly bằng phần cứng sang **Language-Based Isolation (LBI)** để triệt tiêu chi phí IPC.

### Key Differentiators
| Feature | Traditional OS | **Cellos Cellular** |
| :--- | :--- | :--- |
| **Isolation** | Hardware MMU (Slow) | **Compiler/Language (Zero-Cost)** |
| **IPC** | Message Passing | **Direct Function Call** |
| **Kernel Role** | Resource Manager | **Runtime Linker & Manager** |

## 2. The Cellular Model (SAS)
Tất cả chạy trong **Single Address Space (Ring 0)**.

### The "Cell"
* **Dạng vật lý**: File ELF (.o) đã được ký số.
* **Liên kết**: Trực tiếp qua VTable hoặc Symbol Table.

## 3. Nano Kernel: The Construction Site
Kernel tối giản, tập trung vào việc "xây dựng" hệ thống lúc runtime.

### Global Symbol Table (Enhanced)
* **Cấu trúc**: Sử dụng **Lock-free Hash Table** để ánh xạ `SymbolName -> Address`.
* **Tốc độ**: O(1) lookup, đảm bảo nạp hàng trăm Cell trong < 500ms.

### Dependency Management (DAG & Weak Refs)
Để tránh Deadlock khi Unload, Cellos phân loại liên kết:
1.  **Strong Ref**: Cell A không thể sống thiếu Cell B. `ref_count` tăng.
2.  **Weak Ref**: Liên kết tạm thời (như Logging). Không tăng `ref_count`, cho phép Unload Cell đích và trả về lỗi `SymbolNotFound` khi gọi.

## 4. The Gatekeeper & Security
1.  **Signature**: Mọi Cell phải có chữ ký Ed25519 từ Cellos Lab.
2.  **Capabilities (Tokens)**: Sử dụng Zero-Sized Types (ZST). 
    * `fn reboot(_: RebootCap)`.
    * Token chỉ được cấp qua hàm `init()` của Cell và không thể copy trái phép.

## 5. Fault Tolerance (Terminate and Supervise)
Cell builds use abort-on-panic; the kernel does not unwind across Cell boundaries.

1. Panic/trap terminates the faulting Cell through the normal exit path.
2. Reaping releases task-owned frames, grants, pins, and registrations in lifecycle order.
3. Exit notification carries the reason to the supervisor.
4. Supervisor policy decides restart, backoff/intensity, or permanent stop. Driver reset
   and state restore are explicit service policies, not automatic kernel side effects.

## 6. Lifecycle Integrity
* **Hot-swap**: Chỉ cho phép khi không có Strong Ref nào đang hoạt động.
* **Zombie State**: Đánh dấu Cell đang chờ chết, từ chối mọi yêu cầu liên kết mới.

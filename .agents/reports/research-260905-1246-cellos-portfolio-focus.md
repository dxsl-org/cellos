# Cellos: chọn một hướng thay vì dàn trải G1–G3

Ngày: 2026-09-05. Phạm vi: portfolio strategy + market research. Đây là khuyến nghị, không phải quyết định đã được duyệt hay thay đổi roadmap/source.

## Verdict

**[INFERENCE — khuyến nghị] Chọn một chương trình duy nhất: chứng minh giá trị của appliance dịch vụ cục bộ, một chủ sở hữu, có lifecycle/state/resource contract rõ ràng.** Điểm vào cụ thể để kiểm chứng là appliance thu thập/biến đổi/ghi nhận dữ liệu với một adapter dịch vụ có thể cập nhật hoặc hồi phục có kiểm soát. Không mở một platform IoT tổng quát.

- Giữ phần G1 phục vụ đúng appliance đó; lấy cơ chế lifecycle của G2 làm capability, không mở thị trường server/desktop.
- Đưa G3 ra khỏi lịch phát hành chủ động; accelerator chỉ là tùy chọn backend khi một workload/khách hàng đã được xác nhận cần nó và đủ gate phần cứng/pháp lý.
- Không tiếp tục chỉ vì đã đầu tư vào kernel. Linux reference là phép thử bắt buộc của luận điểm native; nếu Linux đáp ứng hợp đồng với TCO thấp hơn thì dừng mở rộng kernel cho luận điểm đó và xem xét sản phẩm hosted theo một thiết kế riêng.
- Không xây native và hosted thành hai platform song song. Trước mắt chỉ làm một native reference hẹp và đối chứng Linux tối thiểu dùng công cụ hiện có; hosted Cellos hiện chưa tồn tại.

Mức tin cậy: cao về các capability/gap có source; trung bình về xếp hạng theo ràng buộc solo và tài sản hiện có; thấp/chưa biết về nhu cầu trả tiền. Không có market share, paid pilot hay comparative production result chứng minh lựa chọn này tối ưu toàn thị trường.

## Scope Contract

- **Output:** một trọng tâm, giải thích vì sao không chọn hai hướng còn lại, bảng giữ/đóng băng/mở lại, và go/no-go theo kết quả.
- **Acceptance:** xét buyer/job, switching cost, khả năng phân phối, dependencies, maintenance, source reuse và release gates; không xếp hạng theo độ mới của công nghệ.
- **Boundary:** không sửa plan cũ, trạng thái lane, ABI, production/security requirements, licence hay code; không mở sensor lane, procurement hoặc cloud.
- **Constraints:** solo maintainer; hai Pi3 B+ là phần cứng phát triển; native Tier1 trusted SAS/no_std, Rust std/PAL chưa shipping; G3 chưa có target/SDK được nhận; production root chưa chọn.
- **Touchpoints:** product-stage overlay, capability lanes, runtime/platform tracks, G3 envelope, ADR-0006/0007/0013, prior SAS/LBI closure plan và báo cáo G1 market trong cùng phiên. [R1–R6]

## Why the Current Portfolio Spreads Effort

### G1–G3 là các hợp đồng sản phẩm khác nhau

| Hướng | Người mua/job | Chi phí mới sau mỗi capability kernel | Điều không tự chuyển từ hướng trước |
|---|---|---|---|
| G1 Robot/Embedded | Embedded/OEM lead: board, I/O, timing, recovery, cập nhật thiết bị | BSP/driver, peripheral, firmware, power/thermal, hỗ trợ thiết bị | G1 không tự tạo ứng dụng server, package ecosystem hay cloud operations |
| G2 Server/PC | Infra/operator hoặc desktop user: chạy phần mềm sẵn có, isolation, storage/network, deploy/debug | x86 physical qualification, compatibility, tooling, fleet/ops; desktop thêm GPU/UI/apps | G2 không tự tạo compiler/runtime/firmware NPU |
| G3 NPU-native | Inference/OEM/SoC team: model chạy đúng, throughput/latency/power, camera/media, lifecycle | Vendor driver/compiler/runtime, exact SDK licence, model compatibility, accelerator memory/reset behavior | Có grants hoặc RISC-V HAL không đồng nghĩa có tensor, NPU driver hay purchasable X390 target |

Roadmap đã quy định đây là overlays, không phải G1→G2→G3 execution queue. Nhưng **được phép chạy một lane không đồng nghĩa nên dành nguồn lực cho lane đó**. Local evidence gates giải quyết correctness/promotion; chúng chưa chọn một buyer/job. [R1]

Ba nguồn phân tán cần sửa về mặt chiến lược:
1. Platform breadth trở thành mục tiêu: thêm ISA/runtime/driver vì có thể thêm, không vì giải một contract khách hàng.
2. Substrate bị dùng thay outcome: guest boot, SDK shim hoặc native task smoke bị kỳ vọng mở ra cả thị trường.
3. Sunk cost: vì đã có hypervisor/UI/grants nên tiếp tục mở rộng chúng, dù giá trị tương lai chưa được chứng minh.

Khi đổi roadmap cần phân biệt program với closure plan: `.agents/plan-portfolio.md` đặt Midori làm sole active program; closure record ngày06-08 ghi chương trình closure complete theo tiêu chí đã sửa, đồng thời giữ một số scope gốc partial/deferred. Closure complete không tự chứng minh master program đã complete hoặc index sai. Nên có một nơi thể hiện ưu tiên sản phẩm hiện hành, dẫn tới đúng execution plans và giữ lịch sử nguyên vẹn. [R7]

## Market Size & Segments

Không cộng TAM embedded OS + server OS + edge AI: các định nghĩa này chồng lấn và không đo thị trường có thể tiếp cận của Cellos. Báo cáo G1 trước đã kiểm tra doanh thu phân khúc QNX FY2026, nhưng số đó không phải RTOS kernel revenue hay SAM của Cellos. Các nguồn mới dưới đây chủ yếu chứng minh capability/substitute, không chứng minh nhu cầu chuyển sang Cellos. [R6]

**[INFERENCE]** Đơn vị thị trường nên là số đội có một job cụ thể, có incident/cost thật, có quyền thay stack và có khả năng cung cấp workload để thử. Một pilot phát triển trả phí là tín hiệu đầu tiên khả dụng; không được gọi đó là production adoption.

| Nhóm buyer | Pain có thể hỏi/đo | Switching cost cần vượt | Quyết định ưu tiên |
|---|---|---|---|
| MCU firmware/low-power | Deadline, flash/RAM/power, SDK silicon | BSP, toolchain, existing RTOS integration | Không mở MCU product để đuổi FreeRTOS/Zephyr |
| Robot/AI application team | Model/perception/control integration, camera/GPU/ROS dependencies | Port middleware, vendor stack và application graph | Không chọn full robot brain; xem một component độc lập nếu có pain |
| Cloud/multi-tenant | Tenant isolation, compatible execution, deployment cost | Security boundary, Linux ecosystem, ops/fleet | Loại khỏi focus hiện tại: trusted SAS không phù hợp arbitrary tenant code |
| OEM appliance local, một chủ | Mất/nhân đôi record, mất phiên/state, thời gian phục hồi/cập nhật, chi phí hỗ trợ | Driver/app port, protocol integration, maintainability và vendor continuity | Giả thuyết đầu tiên để kiểm chứng, không mặc định có người mua |
| Existing Linux platform team | Lifecycle/state/testing burden | New runtime/SDK, application changes; ít BSP migration hơn đổi OS | Đối chứng và nhánh thay thế nếu kernel không tạo giá trị |
| SoC/NPU vendor hoặc integrator có sponsor | Device reset/DMA/authority problem không giải được trên stack hiện tại | Vendor access, legal, co-design và qualified hardware | Chỉ mở lại G3 khi có sponsor và evidence; không dựa vào AI hype |

## Competitor Matrix

### G1: không chọn RTOS tổng quát

G1 mạnh nhất ở khả năng thử một appliance SBC hẹp với trusted Cells, I/O/VFS và lifecycle. Nó yếu ở ecosystem, evidence physical trên exact current boards, production floor và support. Đối thủ không chỉ Linux nguyên bản: PREEMPT_RT, Zephyr, Linux+MCU/RTOS và lifecycle tooling đều tồn tại. [R6][S1]

Nguồn mới làm luận điểm gateway khó hơn: Eclipse Kura cung cấp framework Java/OSGi, dynamic components, device/config/update management và protocol integration; trang chính thức liệt kê Linux packages cho ARM32/64 và x86_64. Không thể lấy “gateway mô-đun có update” làm USP riêng. Trang sản phẩm không chứng minh mọi update giữ state hoặc không gián đoạn trong workload thực. [S2]

**Kết luận G1:** ưu tiên một phần việc hẹp, không toàn bộ robot/RTOS/gateway ecosystem; chỉ đáng tiếp tục nếu outcome thắng đối chứng đủ bù port/support cost.

### G2: tách appliance khỏi server/desktop/cloud

| Đối thủ/thay thế | Capability có source | Điều không được đánh đồng |
|---|---|---|
| Linux + systemd | Supervision, readiness, reload, restart/watchdog và activation | Không tự bảo toàn arbitrary application state; app vẫn phải có protocol |
| KVM/Firecracker | Hardware-virtualized Linux workloads, API quản lý microVM, snapshot memory/device state | Snapshot không tự bảo toàn disk consistency hay mọi connection; không phải per-service authority handover |
| Unikraft/Nanos | Application-specific/unikernel VM image, bounded compatibility envelope | Small specialized image không phải điểm riêng của Cellos; không mặc nhiên có live component update |
| Wasmtime/WASI, SpinKube | Wasm component execution/capabilities và deployment theo Kubernetes | Không cùng trust/format model với native SAS; cũng không là general POSIX OS |
| Erlang/OTP | Release handling, nâng/hạ phiên bản lúc chạy, suspend→state conversion→code replacement→resume | Không tự xử lý mọi external side effect; dependencies và concurrent updates vẫn phức tạp |
| Restate | Durable execution journal, recovery, virtual-object state, service SDKs | Journal/replay khác hot native resource handover; physical side effects cần integration contract, không suy exactly-once tùy ý |

Nguồn: [S3–S8]. **Correction đối với luận điểm trước:** cả state-aware live upgrade cũng có tiền lệ trong Erlang/OTP. Vì vậy không chỉ “restart/OTA” mà cả “stateful hot-swap” đều phải được đánh giá bằng chi phí và outcome cụ thể, không bằng novelty.

Cellos chưa có native Rust std/PAL/sysroot shipping, Tier1 chỉ trusted SAS, x86 evidence chủ yếu QEMU; Linux guest không biến thành native POSIX/Python parity. Do đó generic Rust hosting, cloud sandbox và desktop đều kéo theo hợp đồng mới rất lớn. [R2]

**Kết luận G2:** lấy atomic local service cutover/ownership làm capability của cùng appliance; không mở server/PC product. x86 chỉ thành deployment profile mới sau khi có buyer cần nó và qua qualification tương ứng.

### G3: phân biệt accelerator workload và NPU-native OS

- JetPack cung cấp một stack gồm Jetson Linux/driver/firmware, CUDA/TensorRT và media/vision tooling; đây là ví dụ vendor-owned deployment stack, không bằng chứng Cellos hỗ trợ Jetson. [S9]
- RKNN Toolkit2 tách host model conversion khỏi board runtime/driver; accepted version, licence và binary redistribution là dependency thật. [S10]
- ONNX Runtime đặt graph partitioning, backend capability, allocator và hardware-specific execution ở Execution Providers. Một OS mới không tự giải unsupported operators, precision/quantization, engine formats hay vendor firmware. [S11]
- Không dùng luận điểm tuyệt đối “AI bắt buộc Linux”: Canaan có **K230 RTOS Only SDK dựa trên RT-Smart**, là counterexample chính thức. Điều vẫn giữ là phải có hardware-supported compiler/runtime/driver stack; Cellos hiện không có stack đó. [S12]
- X390 được mô tả là core IP; không thể coi thành một board/SDK có thể dùng ngay cho qualification. [R3][S13]

Amdahl minh họa cơ hội tối ưu: nếu một phần OS chiếm tỷ lệ `f` của tổng thời gian và chỉ phần đó được tối ưu, speedup lý tưởng bị chặn bởi `1/(1-f)`. Với **giả định minh họa**, không phải số đo, `f=10%`, bỏ hoàn toàn overhead đó chỉ đạt khoảng1.111×. Lập luận này không phủ nhận giá trị scheduling/zero-copy/recovery, mà buộc đo đúng phần OS thực sự kiểm soát.

**Kết luận G3:** chỉ mở nếu pain nằm ở lifetime/authority/DMA/reset/lifecycle mà hiện trạng không giải được. Nếu pain là chạy model, port operator hay quantization, một runtime/compiler/adapter trên stack vendor có thể phù hợp hơn thay OS. Hiện G3 envelope đã cấm probe crate/public ABI/scheduler khi còn BLOCKED; giữ nguyên. [R3]

## Trends

Các quan sát dưới đây là facts về sản phẩm/kiến trúc, không phải ước lượng tốc độ tăng adoption:
1. Các lớp đã chồng lấn: Linux có RT profile; RTOS có target MPU; runtimes/VMs có isolation/lifecycle và deploy model riêng. Nhãn OS không xác định một thị trường trống. [S1][S4–S8]
2. Specialization đã có đối thủ: unikernel, microVM, Wasm, edge framework và durable runtime. Cellos phải thắng một cấu hình cụ thể, không thắng một caricature của Linux. [S2][S4–S8]
3. SDK/model ecosystem tạo phụ thuộc thật ở accelerator, cả với Linux và RTOS. Không phải mua NPU board là đã có quyền hoặc năng lực port stack. [S9–S12]
4. Lifecycle/security support có động lực pháp lý, nhưng cũng thêm nghĩa vụ nhà cung cấp. CRA không là bằng chứng buyer muốn một OS mới và không tự chứng minh Cellos compliant. [S14]

## Strategic Options and Ranking

**[INFERENCE]** Xếp hạng định tính, không phải khảo sát thị trường hay scoring xác suất thành công. Tối ưu theo: có thể kiểm chứng nhu cầu, tận dụng phần đã có, tránh dependency không kiểm soát được, giữ maintenance/switching cost trong phạm vi một maintainer. Sunk cost không được tính là lợi ích tương lai.

| Phương án | Reuse hiện tại | Dependency/migration bill | Phép thử giá trị ngắn và rõ | Xếp hạng |
|---|---|---|---|---|
| RTOS/robot OS tổng quát | Một phần G1 | Rất lớn: board/peripheral/middleware | Khó vì nhóm buyer và requirement quá rộng | Không chọn |
| Server/desktop/cloud OS | Một phần x86/VM/SMP | Rất lớn: compatibility/qualification/isolation/ops | Không có một job chung | Không chọn |
| NPU-native OS | Grants/HAL chỉ là tiền đề | Lớn, phụ thuộc vendor/hardware/legal | Chưa thể làm bằng inventory hiện tại | Đóng băng |
| Native local appliance, một service graph/owner | Tương đối trực tiếp: no_std SDK, VFS/IPC/lifecycle | Vẫn đáng kể nhưng có thể giới hạn; production gates còn mở | Có thể định nghĩa một contract/fault experiment cụ thể | Focus kiểm chứng ưu tiên |
| Linux-hosted lifecycle/SDK | Chủ yếu concepts/oracles, không phải port sẵn có | Ít BSP migration hơn; vẫn phải thiết kế runtime/product | Đối chứng tốt; commercial fit chưa biết | Nhánh thay thế, không xây platform song song |
| Evidence/verification tooling | Reuse validator/provenance/oracle ideas | Cần generalization, integration, support và buyer mới | Có thể là deliverable phụ của pilot | Chưa mở thành business độc lập |

Nếu mục tiêu bắt buộc là **production revenue sớm**, current native qualification gates làm native appliance chưa có đường shipment đã chứng minh; Linux-hosted software hoặc paid evaluation/R&D có thể có logic tốt hơn. Nếu mục tiêu bắt buộc là **OS research**, giữ native nhưng đo bằng đóng góp kỹ thuật, không gắn market fit chưa có. Không mặc định hai mục tiêu này giống nhau.

## Chosen Focus: One Local Appliance Outcome

Tên làm việc: **Cellos appliance cho xử lý dữ liệu liên tục và lifecycle có kiểm chứng**. Đây là một job cụ thể, không phải mở platform IoT/robot/cloud mới.

### Initial customer hypothesis

Embedded/OEM lead sở hữu toàn bộ phần mềm một thiết bị thu thập/biến đổi/ghi nhận dữ liệu. Đội đó có chi phí thật khi đổi phiên bản protocol adapter hoặc hồi phục dịch vụ làm mất record/phiên, nhưng không bắt buộc full POSIX/GPU/ROS hay arbitrary third-party native code. Chọn một lớp thiết bị và một protocol sau khi có buyer evidence; hiện chưa có khách hàng được xác nhận.

### Reference workload hypothesis

`input records → adapter/decoder → validation/transformation → checkpoint/log → output + acknowledgement`

- Native reference có một adapter trusted; cập nhật từ schema/versionA sangB trong khi các component không liên quan vẫn chạy.
- Contract định nghĩa thứ tự/ack watermark, failure modes có thể hồi phục, queue/backpressure, output semantics, quyền truy cập tài nguyên và reauthorization sau replacement.
- Không hứa mở POSIX FD, pending future, IRQ hay mọi external session sống xuyên hot-swap. VFS handle cũ cần reissue theo lifecycle hiện hành. [R2][R5]
- Không gọi acknowledged checkpoint là bảo đảm arbitrary power-loss durability; storage acknowledgement và persistence boundary phải được định nghĩa/thử riêng.
- Input generator có thể dùng cho software proof trước; đó không phải sensor/field/production proof. Muốn dùng sensor hiện deferred phải mở lại đúng lane.

### Why it is one direction rather than G1+G2+G3 again

G1 cung cấp form factor/I/O; một phần G2 cung cấp service lifecycle; G3 không tham gia. Chỉ những capability cần cho contract mới được mở. Không chạy một chương trình native SDK tổng quát, cloud orchestration hay accelerator abstraction để “chuẩn bị tương lai”.

### What must be better

Ít nhất một yếu tố phải làm thay đổi quyết định mua/thiết kế: giảm chi phí integration/support; đạt interruption/recovery/data-integrity requirement mà đối chứng không đạt hợp lý; hoặc giảm resource/BOM ở mức có ý nghĩa. P99 đẹp, ít LOC hay một opcode mới không tự đủ.

Linux reference phải công bằng: dùng readiness/drain + state protocol hoặc runtime phù hợp như OTP khi buyer chấp nhận, không cố ý so Cellos stateful với một process bị kill mà không có recovery design. Nếu đối chứng đạt contract với TCO thấp hơn, không tiếp tục mở kernel feature cho luận điểm đó.

## Proposed Roadmap Cutover — Requires Approval

Bảng này là **đề xuất**, không ghi đè acceptance của plan đã duyệt.

| Hạng mục | Quyết định đề xuất | Điều kiện làm/mở lại |
|---|---|---|
| Benchmark validity, authority, grant/lifetime/quota, supported-path security | Giữ; fix/reproduce theo scope hiện hữu | Không dùng focus mới làm lý do bỏ known safety/correctness defects |
| Stateful hotswap | Giữ contract correctness của path đang hỗ trợ; không mở generic live-migration framework | Deep extension chỉ khi reference workload/customer cần; authority/stash/fence gates không bỏ |
| VFS/IPC, bounded queues, error paths, observability tối thiểu | Giữ đúng phần reference cần | Consumer-observable acceptance, không tooling framework vô hạn |
| G1 full robot/RTOS/MCU expansion | Đóng băng mở rộng; giữ một appliance/profile | Named buyer/workload chứng minh phụ thuộc cụ thể |
| Board breadth | Một physical development profile khi cần; RV64 QEMU là regression/reference không phải SKU thứ hai | Board mới chỉ khi buyer/sponsor và exact qualification plan |
| x86/SMP/storage expansion | Giữ regression phần đã có; không scale program độc lập | Cùng appliance thực sự cần x86/throughput/storage lớn |
| Desktop/compositor/UI/GPU | Giữ code/evidence regression cần thiết; không thêm UX/product breadth | Customer outcome có UI requirement đã chọn, không vì đã có ViUI |
| G3 NPU/ViAccelerator/tensor scheduler | Park, giữ hiện trạng hardware/legal gate | Buyer pain + target/SDK/licence + vendor baseline + measured OS-level gap; public ABI vẫn cần hai implementation và approvals |
| G4 native Rust std/POSIX breadth | Không làm thành blanket prerequisite | Workload/crate bắt buộc và scope được duyệt, vẫn giữ PAL/security approvals |
| G5/VM/Linux compatibility expansion | Giữ current regression; chỉ dùng guest khi thật cần và qualified | Không dùng “boot Linux bên trong” để mặc nhiên mở toàn bộ ecosystem/desktop |
| Remote/public C2C, distributed orchestration | Không mở làm product program hiện tại | Paid/validated remote requirement và toàn bộ identity/service gates |
| Production root/secure identity | Giữ kill gates và evidence; không tạo placeholder hardware | Exact vendor/product evidence, approvals và funded qualification; không giảm yêu cầu để ship nhanh |
| Authenticated evidence/CI | Giữ năng lực đã closed và regression | Mở khi có regression hoặc đúng promotion claim cần đạt |

Park nghĩa là không tăng scope; không xóa code, không xóa historical evidence, không drop regression cần cho supported paths. Không coi mọi future capability là bug/debt phải hoàn tất trước một pilot.

## Outcome-Driven Execution Gates

Không lập lại roadmap theo số năm hay số G. Đề xuất bốn cửa ra vào:

**A — Problem selection.** Một buyer persona, một incident/workload, một owner quyết định stack. Phỏng vấn có bằng chứng vận hành; ngưỡng gợi ý là3 đội cùng pain,2 workflow để thử,1 đối tác nhận pilot. Đây là ngưỡng đề xuất chứ không số liệu đã có. Không thêm features trước khi biết requirement thật.

**B — Paired reference.** M0/M1 correctness hỗ trợ một native reference hẹp và Linux comparison. Chốt D/J/R/resource/output contract theo workload, cả acceptance và negative cases. Counter/RedoxFS plan cũ là substrate proof, không tự thành field-product evidence; thay scope của plan đó phải được duyệt.

**C — Partner evaluation.** Reproduce bằng dữ liệu/workflow đối tác; đo acknowledgement correctness, bounded recovery, operator steps, ported code/dependencies, memory/latency/error và integration burden. Không chỉ benchmark successful path. Paid evaluation là research/development deliverable, không tự là production-qualified deployment.

**D — Product and shipment decision.** Chỉ productize khi benefit vượt migration/support/qualification costs. Chốt exact board, licence, support responsibility và khả năng đáp ứng production gates trước khi hứa shipment. Không hoàn thành được các điều kiện này thì chưa có native product business case, dù demo đã chạy.

Sau khi quaC, chỉ mở **một** capability kế tiếp do cùng customer/job kéo. Không mở x86 và NPU đồng thời chỉ vì đều có thể dùng lại grants.

## Work-In-Progress and Stop Rules

- Một product outcome + một supporting correctness/evidence lane; P0/security có thể preempt. Không cấp mặc định một nhóm/todo track cho mỗi G.
- Mỗi việc mới phải sửa defect của supported path, đóng acceptance của outcome đã chọn, hoặc thử một giả định có thể làm đổi quyết định. Không thuộc ba loại này thì park.
- Review ưu tiên theo insight/outcome, không theo số commit, crates, board targets hay tổng PASS.
- Không có business validation không đồng nghĩa bỏ bảo trì; nó có nghĩa không tăng platform breadth.
- **Kill native differentiation thesis** nếu Linux/runtime incumbent đáp ứng cùng contract với lower TCO và không có resource/deadline/authority advantage đủ bù đổi OS.
- **Reopen G3** nếu sponsor cung cấp target/vendor cooperation và vấn đề nằm đúng OS-controlled lifecycle; không vì mua được board, có benchmark TOPS hay AI đang được quan tâm.
- **Reopen G2 platform** chỉ khi cùng appliance có khách hàng x86 cụ thể; desktop/cloud multitenancy là thesis khác cần lựa chọn lại, không feature tiếp nối tự động.
- Không đủ customer signal: giữ Cellos ở phạm vi R&D có thước đo kỹ thuật, hoặc chọn hosted/tooling thesis riêng sau phê duyệt; không tô lại backlog thành một thị trường khác.

## Risks and Claim Boundaries

1. **Commercial demand UNVERIFIED:** chưa có interview, pilot, WTP hay objective cost comparison. Một market thesis hẹp vẫn có thể không có người mua.
2. **Production path blocked:** ADR-0006 chưa chọn exact production root; Pi3 không đáp ứng non-circular production boot/root requirement. Root/admission/approval gaps không block unrelated development, nhưng block lời hứa production tương ứng. [R4]
3. **Threat-model mismatch:** trusted SAS không là sandbox cho third-party native code. Không bán “fault isolation” cho arbitrary malicious native/DMA code từ các proof service-level hẹp. [R2]
4. **SDK switching cost:** native no_std không phải normal Rust/POSIX environment; hosted direction chỉ tái dùng được concepts/oracles cho đến khi thiết kế/reuse inventory được duyệt. Tier3 Linux dưới Cellos hoàn toàn khác Cellos chạy trên Linux.
5. **Maintenance compounds:** thêm ISA, runtime, peripheral, filesystem và trust mode tạo thêm tương tác phải duy trì; shared kernel code không xóa chi phí qualification theo deployment.
6. **Root-gate arbitrage prohibited:** chuyển sang Linux-hosted không tự miễn security/update obligations; không dùng một tên deployment khác để lách claim/approval của Cellos-native. Nếu sản phẩm hosted khác thật, nó cần contract và approval riêng.
7. **Prototype versus production:** no-safety-critical scope không đồng nghĩa miễn security requirements; một paid lab evaluation không được dùng làm production rollout.

## Active Refutation and Unresolved Questions

- “Hot stateful upgrade chưa ai làm”: **REFUTED ở nghĩa rộng**, OTP có documented state conversion/release handling. Cellos chỉ còn hypothesis về native authority/resource semantics và tổng chi phí trên một workload. [S7]
- “NPU inference bắt buộc Linux nên OS khác vô ích”: **REFUTED ở nghĩa tuyệt đối**, K230 có RTOS Only SDK. Điều chưa có ở Cellos là supported stack và paid OS-level problem; không sửa kết luận park hiện tại. [S12]
- “Shared SAS + snapshot đủ cạnh tranh cloud runtime”: **không được evidence hỗ trợ**; Firecracker/Wasmtime có boundary/migration tradeoffs khác, còn Tier1 của Cellos loại trừ arbitrary untrusted native code. [R2][S4][S6]
- “Local appliance lifecycle đáng để đổi OS”: **UNVERIFIED**; Kura/OTP/Linux/runtime alternatives khiến phép thử TCO càng cần thiết.
- User ưu tiên OS research, commercial revenue hay một deployed appliance cụ thể đến mức nào? Báo cáo chọn product-validation-first, giữ native có điều kiện thay vì mặc định kernel phải là đơn vị bán.
- Buyer/protocol/driver/safety/deadline thực tế và khả năng đáp ứng exact production-floor contract chưa xác nhận. Không thể tuyên bố tìm ra optimum toàn thị trường từ desk research.

## Sources

### Repository and prior evidence
- **[R1]** `docs/project-roadmap.md` §Capability Lanes/Current Direction; `docs/roadmap/product-stages.md` §§Execution Relationship/G1–G5; `docs/roadmap/current-focus.md`.
- **[R2]** `docs/app-development-guide.md:21-45`; `docs/roadmap/runtime-and-platform-tracks.md` §§Tier1 Rust std Feasibility; `docs/security-model.md`; `docs/hotswap-guide.md` và VFS handle lifecycle trong `docs/specs/09-vfs.md`.
- **[R3]** `docs/research/g3-accelerator-evidence.md` §§Provenance/Existing Large-Buffer Substrate/Promotion Gates; `docs/specs/04-hardware.md`.
- **[R4]** `docs/decisions/0006-block-production-root-pending-exact-product-evidence.md`; `docs/decisions/0013-solo-first-development-independent-promotion.md`; `docs/roadmap/open-risk-register.md`.
- **[R5]** `.agents/260905-1139-sas-lbi-outcome-closure/plan.md` và phase03/05. Các requirement là kế hoạch, không phải runtime advantages đã đạt.
- **[R6]** [G1/RTOS market research cùng phiên](research-260905-1223-cellos-g1-rtos-market.md), gồm nguồn FreeRTOS/Zephyr/Linux RT/QNX, market-data limitations và production boundaries.
- **[R7]** `.agents/plan-portfolio.md:11-17` so với `.agents/260805-1833-midori-closure-execution/plan.md:20-27`; không đổi lịch sử hoặc trạng thái trong nghiên cứu này.

### External primary capability sources
- **[S1]** [Linux PREEMPT_RT](https://raw.githubusercontent.com/torvalds/linux/master/kernel/Kconfig.preempt), [Zephyr introduction](https://docs.zephyrproject.org/latest/introduction/index.html), [TI Linux/RTOS IPC](https://software-dl.ti.com/processor-sdk-linux/esd/AM62X/latest/exports/docs/linux/Foundational_Components_IPC62x.html).
- **[S2]** [Eclipse Kura](https://eclipse.dev/kura/). Read mặc định trả feed rỗng; đã khôi phục raw HTML và kiểm tra phần capability/platform. Không dùng vendor adjectives/adoption claims làm market evidence.
- **[S3]** [systemd.service](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html).
- **[S4]** [Firecracker design](https://github.com/firecracker-microvm/firecracker/blob/main/docs/design.md), [snapshot support and limits](https://github.com/firecracker-microvm/firecracker/blob/main/docs/snapshotting/snapshot-support.md).
- **[S5]** [Unikraft architecture](https://unikraft.org/docs/internals/architecture), [Nanos Book](https://nanos.org/thebook).
- **[S6]** [Wasmtime security](https://docs.wasmtime.dev/security.html), [Spin Operator](https://github.com/spinkube/spin-operator/blob/main/README.md).
- **[S7]** [Erlang/OTP release handling](https://www.erlang.org/doc/system/release_handling.html): state conversion và upgrade/downgrade; có complexity/caveat rõ ràng.
- **[S8]** [Restate key concepts](https://docs.restate.dev/foundations/key-concepts): journal, state, execution recovery và SDK; không sử dụng slogan exactly-once để kết luận về arbitrary physical side effects.
- **[S9]** [NVIDIA JetPack introduction](https://docs.nvidia.com/jetson/jetpack/introduction/index.html), [Jetson software architecture](https://docs.nvidia.com/jetson/archives/r36.4/DeveloperGuide/AR/JetsonSoftwareArchitecture.html), [module/dev-kit lifecycle](https://developer.nvidia.com/embedded/lifecycle). Versioned examples không được gọi là latest release toàn portfolio.
- **[S10]** [RKNN Toolkit2](https://github.com/airockchip/rknn-toolkit2), [SDK licence](https://github.com/airockchip/rknn-toolkit2/blob/master/LICENSE).
- **[S11]** [ONNX Runtime execution providers](https://onnxruntime.ai/docs/execution-providers/), [architecture](https://onnxruntime.ai/docs/reference/high-level-design.html). Không suy RKNPU EP preview/RK1808 thành RK3588 support.
- **[S12]** [Canaan K230 RTOS Only SDK](https://github.com/kendryte/k230_rtos_sdk): official README xác định RT-Smart/K230 SDK. Không dùng stars để suy adoption, không import code/firmware.
- **[S13]** [SiFive X390/core-IP family](https://www.sifive.com/cores/intelligence-x300-series).
- **[S14]** [European Commission CRA](https://digital-strategy.ec.europa.eu/en/policies/cyber-resilience-act), nguồn và mốc hiệu lực đã đọc trong nghiên cứu trước.

## Verification and Deliverable Limits

Ba scout read-only đóng góp asset/G2/G3 evidence; Main đọc roadmap/gates, kiểm tra đối thủ lifecycle và non-Linux accelerator counterexample, tổng hợp và chọn focus. Sources chứng minh capability và constraints, không phải customer willingness. Đã chạy phép tính Amdahl minh họa; không benchmark Cellos/đối thủ, không gọi pilot hay chạy hardware mới. Chỉ tạo báo cáo này; mọi roadmap cutover, hosted design, implementation, procurement và promotion còn cần quyết định riêng.

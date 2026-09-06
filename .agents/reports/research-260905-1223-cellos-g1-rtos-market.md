# Cellos G1: có nên định vị thành RTOS?

Ngày nghiên cứu: 2026-09-05. Loại: market + architecture positioning. Phạm vi: quyết định thị trường, không triển khai hay thay đổi roadmap đã duyệt.

## Verdict

**Không nên pivot thành một RTOS đa dụng chỉ để có lợi thế cạnh tranh.** RTOS là loại sản phẩm/cam kết kỹ thuật, không tự tạo lý do chuyển đổi. **[INFERENCE]** Hướng đáng kiểm chứng cho một OS độc lập là một G1 rất hẹp trên MPU/SBC: xử lý I/O có yêu cầu thời gian rõ ràng, tài nguyên có giới hạn, phục hồi/cập nhật thành phần với state và authority được kiểm chứng. Đây là giả thuyết sản phẩm, chưa phải lợi thế hiện có.

Nếu ưu tiên thương mại hơn việc sở hữu kernel, phải đưa **runtime/SDK trên Linux** vào cùng phép thử. Nếu Linux đáp ứng cùng hợp đồng vận hành với chi phí tích hợp thấp hơn thì kết quả đúng là làm sản phẩm trên Linux, không tiếp tục viết kernel để bảo vệ một quyết định kiến trúc.

### Scope contract
- Output: định vị G1, phân khúc ưu tiên/loại trừ, đối thủ thực tế, điều kiện go/no-go.
- Acceptance: phân biệt capability với lợi thế đã đo và willingness-to-pay; không dùng market share, latency hay TAM không có nguồn làm kết luận.
- Boundary: không sửa kernel, ABI, plan cũ, nhãn sản phẩm, production gates hay giấy phép; không mở sensor/hardware lane đang deferred.
- Constraints: G1 hiện chủ yếu ARM64/RV64 SBC có MMU; maintainer solo; hai Pi3 B+ là tài sản phát triển, không phải bằng chứng khách hàng hay qualified product.
- Touchpoints: `docs/roadmap/product-stages.md:39-50`, `docs/project-overview-pdr.md:14-17,392-401,532-540`, `LICENSE`, và plan SAS/LBI outcome closure đã lập trong phiên.

## Current Cellos Reality

- PDR đã nêu **bounded real-time**, never-die, peripheral I/O và fast boot cho G1; primary target là SBC có MMU, MCU RV32 <512KB chỉ là sub-track. Định hướng real-time vì vậy không phải hoàn toàn mới; điều thiếu là hợp đồng và evidence theo khách hàng/target.
- RTOS không đồng nghĩa MCU: Zephyr liệt kê ARMv8-A/RV64, còn QNX là ví dụ RTOS cho hệ MPU/SBC phức tạp. Hỗ trợ ISA không chứng minh exact-board BSP hay driver phù hợp. [S1][S4]
- Plan trước đã ghi nhận heap reservation 32MiB trên source được khảo sát và các gap benchmark, ownership, stash/authorization/rollback. Không được lấy các bản sửa còn ở kế hoạch làm USP. Đây không phải kết luận rằng kiến trúc không thể thu nhỏ; chỉ nói cấu hình hiện tại chưa là sản phẩm MCU.
- PDR ghi mục tiêu uptime 99.5%. Quy đổi trên năm365 ngày: `(1 - 0.995) * 365 * 24 = 43.8 giờ`. Đây là phép tính trên **mục tiêu**, không phải downtime đã đo. “Never-die” cần chuyển thành failure model, detection/recovery bounds và trạng thái an toàn, không phải slogan.
- `LICENSE` dùng MPL-2.0 cho kernel/HAL, MPL + linking exception cho ostd và các giấy phép khác theo lớp. Không nên mô tả toàn bộ Cellos đơn giản là permissive. FreeRTOS MIT và Zephyr Apache-2.0 đã tồn tại; licensing openness không phải khoảng trống riêng. [S1][S2]

## Market Size & Segments

### Quy mô: bằng chứng nào dùng được?

Bảng kết quả tài chính BlackBerry công bố cho năm tài chính kết thúc **28-02-2026** ghi doanh thu phân khúc QNX **268.0 triệu USD**, so với236.0 triệu USD năm trước. Đây là doanh thu phân khúc gồm nhiều sản phẩm/dịch vụ QNX, **không** phải thị trường RTOS toàn cầu, không phải doanh thu kernel thuần, cũng không phải TAM/SAM/SOM của Cellos. Nguồn: báo cáo kết quả FY2026 ngày09-04-2026, bảng phân khúc trang10. [S8]

Các kết quả tìm kiếm sizing RTOS cho con số chênh lệch lớn và phạm vi không đồng nhất. Không chọn một con số tỷ USD rồi suy ra “lấy1% là đủ”. Không có số market share hay ước tính doanh thu Cellos đáng tin cậy trong nghiên cứu này.

Survey Eclipse2025 được công bố tháng03-2026, nhưng tài liệu công khai đã đọc dẫn tới trang download/account và không cung cấp bảng số liệu cùng phương pháp lấy mẫu. Không suy ra tỷ lệ Linux/FreeRTOS/Zephyr từ các tóm tắt tìm kiếm. [S10]

**[INFERENCE]** Với dự án solo, mô hình thị trường có ích hơn là bottom-up: số đội OEM thực sự tiếp cận được × xác suất chuyển thành pilot trả phí × giá trị hợp đồng có thể giao được, trừ engineering/BSP/support obligations. Chưa có dữ liệu khách hàng để điền các biến này.

### Phân khúc và mức phù hợp

Các đánh giá ở cột cuối là **[INFERENCE]**, không phải kết quả khảo sát khách hàng.

| Phân khúc | Buyer/job thực tế | Lựa chọn sẵn có | Phù hợp G1 hiện tại |
|---|---|---|---|
| MCU chi phí thấp, pin, motor/sensor control | Firmware lead: đáp ứng deadline, flash/RAM/power và SDK chip | Bare metal, FreeRTOS, Zephyr, các MCU RTOS khác | Thấp: khác target chính, thêm BSP/toolchain/power-management surface. Không nên đuổi generic feature parity. |
| Robot brain/edge application nhiều middleware | Robotics/software lead: chạy ứng dụng, perception/UI/networking, tích hợp thiết bị | Linux/PREEMPT_RT; Linux + RTOS controller | Thấp nếu đòi chuyển toàn bộ stack ứng dụng/driver sang Cellos. Chỉ nên nhận phần việc nhỏ có boundary rõ. |
| Thiết bị regulated/safety-critical | OEM/safety lead: evidence, traceability, tool qualification, lifecycle | QNX OS for Safety, VxWorks Cert Edition và hệ đã qualified theo sản phẩm | Không làm beachhead hiện tại. Certification không bắt buộc cho mọi RTOS, nhưng là yêu cầu riêng của phân khúc này. |
| Appliance I/O/control-plane trên SBC, không safety-critical | Embedded lead của OEM: giảm mất phiên/state, thời gian hồi phục, công sức bảo trì | Embedded Linux + services/update tools, Zephyr khi BSP phù hợp | Đáng kiểm chứng: greenfield, protocol nhỏ, không lệ thuộc nặng POSIX/GPU/vendor SDK. Không mặc định có nhu cầu hot-swap. |
| Đội Linux đã có pain về lifecycle/state | Platform lead: nâng cấp/recover component mà không port OS | systemd + protocol ứng dụng + RAUC/Mender; runtime/SDK mới | Đáng kiểm chứng cho thương mại; có thể dẫn đến sản phẩm Linux-hosted chứ không dẫn đến adoption kernel Cellos. |

## Competitor Matrix

Nguồn chính thức chứng minh capability/sản phẩm được cung cấp; không tự chứng minh market share hay willingness-to-switch.

| Đối thủ/thay thế | Capability có nguồn | Rào cản đối với Cellos | Phân biệt cần giữ |
|---|---|---|---|
| FreeRTOS | MCU/small microprocessor; MIT; kernel và thư viện connectivity/security/OTA. [S2] | Đã có nền tảng FOSS và tích hợp phần cứng; miễn phí không đủ để đổi. | Không suy từ RTOS/kernel sang bảo đảm hard-RT toàn hệ hoặc chứng nhận safety. |
| Zephyr | Kernel cấu hình được, scheduler, networking/device model; ARMv8-A và RV64; Apache-2.0; release/LTS policy công khai. [S1] | Đối thủ FOSS trực tiếp cả ở mô hình RTOS phong phú, không chỉ MCU nhỏ. | ISA support ≠ exact Pi3 B+ support; LTS ≠ SLA; safety initiative ≠ mọi Zephyr build đã certified. |
| Linux/PREEMPT_RT | Mainline có CONFIG_PREEMPT_RT; locking/IRQ behavior được thay đổi; cấu hình còn phụ thuộc architecture support. [S3] | Có thể giữ Linux apps/drivers/tooling, thay vì port OS. | Hardware/firmware, DMA/cache/bus/driver và cấu hình vẫn ảnh hưởng timing; không có deadline chung cho mọi board. |
| Linux + MCU/lõi RTOS | Linux remoteproc/RPMsg; TI AM62x có A53 Linux + remote RTOS cores và reload một số core. [S5] | Có cách tách control deadline khỏi application stack mà không bỏ Linux. | Có chi phí dual-firmware/IPC; stop/load/start không phải live stateful handover. Pi3 B+ không mặc nhiên có topology AM62x. |
| QNX/VxWorks | Safety offerings/certification artefacts, SDK/toolchain và dịch vụ lifecycle. [S4] | Buyer mua giảm rủi ro integration/assurance/support, không chỉ scheduler. | Dùng sản phẩm được certified không tự certified toàn thiết bị; vendor PR về adoption không phải market data độc lập. |
| systemd + RAUC/Mender | Watchdog/restart, signed image deployment, boot/rollback workflows và lifecycle hooks. [S6] | “Tự restart” hay “OTA rollback” đã có thể làm trên Linux. | A/B image update/reboot không đồng nghĩa thay component đang chạy mà giữ state/in-flight requests. |
| Hubris/Tock | Rust-oriented embedded OS; Hubris có isolation, IPC, supervisor/component restart. [S7] | Rust, modularity, restart không độc quyền. | Hubris chủ yếu32-bit MCU, không cập nhật task riêng lúc chạy; không phải cùng sản phẩm SBC. |

## Trends

1. **[VERIFIED technical] Real-time và Linux không còn là hai nhãn loại trừ nhau.** PREEMPT_RT là lựa chọn trong mainline; vẫn cần qualifying target/workload. Không lấy “Linux không real-time” làm nền định vị. [S3]
2. **[VERIFIED technical] Phục hồi/cập nhật có nhiều lớp khác nhau.** Process restart, firmware reload, A/B boot rollback và stateful hot-swap không tương đương. Phải chọn đúng lớp mà buyer đang đau. [S5][S6][S7]
3. **[VERIFIED regulatory] Maintenance có động lực chính sách.** EC ghi CRA reporting obligations từ11-09-2026 và main obligations từ11-12-2027 cho phạm vi sản phẩm áp dụng; luật nhấn mạnh vulnerability handling trong lifecycle. Không diễn giải thành mọi dự án OSS đều có nghĩa vụ giống OEM hoặc Cellos đã compliant. [S9]
4. **[INFERENCE]** Đây vừa là cơ hội cho tooling/evidence vừa là gánh nặng đối với nhà cung cấp nhỏ. Chưa có bằng chứng xu hướng đó làm khách hàng chọn Cellos thay incumbent.

## Three Strategic Options

### A. RTOS đa dụng / cạnh tranh MCU trực diện — không khuyến nghị

Phải mở thêm target, BSP, SDK, driver/power integration và developer ecosystem để đuổi lựa chọn đã sẵn có. Tên RTOS không tạo lợi ích mua hàng. Một MCU variant tương lai phải có khách hàng/hardware budget riêng; không lấy nó làm lý do trì hoãn hoặc nhân đôi G1 SBC.

### B. G1 độc lập, RTOS-class cho một appliance SBC cụ thể — giả thuyết ưu tiên nếu phải giữ OS riêng

**Định vị đề xuất, không phải claim hiện tại:** “Nền tảng cho appliance I/O/control trên SBC: hợp đồng timing/tài nguyên theo profile, phục hồi/cập nhật thành phần với state và quyền truy cập được kiểm chứng.”

Điểm bán phải trở thành outcome, chẳng hạn:
- Service không critical lỗi nhưng luồng I/O chính vẫn đáp ứng hợp đồng đã định nghĩa.
- Recovery/cutover không mất hay nhân đôi output đã được xác nhận trong mô hình side effect được hỗ trợ.
- Deadline/jitter/recovery budgets được xác định theo ứng dụng, exact hardware/configuration và tải nền.
- Ít RAM/BOM hoặc công tích hợp hơn đối chứng ở mức đủ làm thay đổi quyết định sản phẩm.

**[UNVERIFIED]** Chưa có bằng chứng Cellos thắng các tiêu chí này hoặc buyer muốn trả tiền. Không hứa “zero downtime” cho mọi lỗi, arbitrary live patch, driver/DMA corruption hay mọi ứng dụng native trong trusted SAS.

### C. Runtime/SDK lifecycle trên Linux — đối chứng thương mại bắt buộc

Nếu pain chính là stateful recovery/update chứ không phải constraint bắt buộc ở kernel, triển khai giải pháp trên Linux có thể giảm switching cost nhiều hơn một OS mới. Đây là đề xuất đánh giá, không phải quyết định bỏ Cellos hay kế hoạch port đã được duyệt.

**Falsifier của B:** Linux reference với explicit state protocol + existing supervision/deployment đạt cùng requirement với tổng chi phí/rủi ro thấp hơn. Khi đó moat nằm ở runtime/SDK/domain tooling, không nằm ở sở hữu kernel.

## What Calling G1 an RTOS Would Commit To

- Phân biệt soft/firm/hard real-time bằng hậu quả deadline miss và hợp đồng ứng dụng; không bằng một ngưỡng microsecond áp dụng chung.
- p99 đẹp hoặc observed maximum nhỏ **không** chứng minh worst-case response bound. Hard-RT cần phân tích response time/assumptions cùng evidence trên target, gồm IRQ blocking, priority inversion, locks, allocator, IPC queues, DMA/cache/bus/firmware, overload và fault behavior. [S3]
- Có thể bắt đầu với profile hẹp: critical path không bị hoạt động cấp phát không chặn được, I/O không giới hạn hay recovery nền phá deadline. Việc quiesce/update phải tương thích hợp đồng control; nếu không, update phải vào maintenance/safe-state window.
- Chứng nhận safety là yêu cầu riêng theo thị trường, không phải định nghĩa RTOS. Không thể mượn từ “Rust” để bỏ qua FFI/driver/TCB, toolchain và qualification evidence.
- Prototype/RPi development proof không phải production floor. Chưa có evidence thì dùng “real-time-oriented prototype”, không quảng cáo hard-RT guarantee.

## Risks & Economics

**[INFERENCE]** Điều kiện chuyển đổi nên viết bằng TCO/rủi ro:

`lợi ích vận hành + BOM + engineering được tiết kiệm > chi phí port + kiểm chứng lại + đào tạo + support + rủi ro nhà cung cấp`.

Cả hai bên đều phải có đối chứng: dùng free kernel không làm BSP, debugging, updates và incident response miễn phí.

Rủi ro lớn:
1. Greenfield niche vẫn có Zephyr/Linux; chưa có bằng chứng về khoảng trống có người mua.
2. Cần ROS2, GPU/NPU vendor SDK, broad POSIX hay binary Linux compatibility thì platform migration có thể lấn át mọi lợi ích hẹp. Không biến các dependency đó thành roadmap parity vô hạn.
3. Hotswap làm tăng state-schema/resource/authority/rollback complexity; khách hàng chấp nhận restart có thể thích thiết kế đơn giản hơn.
4. Solo maintainer là rủi ro continuity/support đối với OEM. AI/CI không thay thế accountable vendor hay cam kết support có năng lực thực hiện.
5. Bán integration/NRE, BSP profile, validation toolkit hay support có thể là mô hình khởi đầu; đây là hypothesis, chưa phải pricing/WTP đã kiểm chứng. Không hứa24/7 hoặc vòng đời nhiều năm khi chưa có năng lực/tài chính.
6. Defect closure M0/M1 là điều kiện tối thiểu. Sửa comparator/grant/rollback không tự chứng minh market differentiation.

## Validation and Kill Criteria

Các ngưỡng dưới đây là **đề xuất go/no-go**, không phải thống kê thị trường hay công việc đã thực hiện.

1. **Buyer gate:** phỏng vấn10–15 embedded/platform leads có quyền chọn stack; hỏi incident thật, downtime/data-loss cost, deadline, BSP, maintenance window, procurement và budget. Không hỏi chung “anh có thích Rust OS không?”.
2. **Pain gate:** ít nhất3 đội mô tả cùng một pain có chi phí đo được; ít nhất2 đội cung cấp workflow/log/board requirements để thử. Ít nhất1 pilot trả phí là tín hiệu mạnh hơn lượt star/download, nhưng chưa chứng minh thị trường lớn.
3. **Reference gate:** chốt một appliance/workload, một board/profile và requirement D (deadline), J (jitter), R (recovery), semantics dữ liệu đã ack, resource/BOM ceiling. Loại trừ safety-critical cho pilot hiện tại; không tự mở sensor lane.
4. **Substitution gate:** đối chiếu Cellos với Linux/PREEMPT_RT cấu hình hợp lý và, khi phần cứng/bài toán phù hợp, Zephyr hoặc Linux+RTOS. Workload và tiêu chí bằng nhau; khác hardware phải so system-level BOM/power, không ghép context-switch numbers.
5. **Failure gate:** thử overload, service crash, restart, update/cutover và cleanup; đo output và ownership, không chỉ log PASS. State continuity phải giữ theo hợp đồng; không gọi replay side effect tùy ý là exactly-once.
6. **Commercial gate:** lợi ích phải đủ vượt switching cost. Nếu lợi ích chỉ là vài microseconds không ảnh hưởng deadline/BOM/operator cost, không xem đó là moat.
7. **Kill/scope rule:** nếu Linux-hosted solution thắng TCO và đáp ứng requirement, ưu tiên sản phẩm trên Linux; nếu buyer chấp nhận reboot và không có timing/resource pain, bỏ luận điểm hot-swap là USP. Nếu tất cả buyer cần ecosystem ngoài khả năng G1, đổi phân khúc thay vì viết tiếp compatibility layer vô hạn.

Plan kỹ thuật trước vẫn hữu ích cho correctness và evidence. Đề xuất bổ sung business validation trước khi mở rộng M2/M3/MCU; chưa sửa plan đó trong lượt nghiên cứu này.

## Active Refutation and Unresolved Questions

- “Gọi RTOS sẽ làm Cellos có lợi thế”: **[UNVERIFIED market]**; bị phản chứng ở luận điểm thiếu đối thủ bởi Linux RT, Zephyr và commercial RTOS. Không có customer-switch evidence.
- “Recovery/update là khoảng trống chưa ai làm”: **[CONTESTED]**; systemd/RAUC/Mender/Hubris có phần đáng kể. Chúng không tự chứng minh đã giải quyết hợp đồng live stateful handover hẹp của Cellos.
- “Stateful recovery/timing trên SBC đủ để khách hàng bỏ Linux”: **[UNVERIFIED]**. Chưa có interviews, paid pilot hay comparative benchmark; bounded searches không thể thay dữ liệu này.
- G1 có phải bắt buộc là OS độc lập, hay mục tiêu tối cao là sản phẩm/doanh thu? Báo cáo đưa cả hai nhánh thay vì tự quyết định thay người dùng.
- Exact buyer/domain/deadline/supported side effects và khả năng port ứng dụng chưa được chốt.
- Không xác minh được bảng số liệu/methodology của survey công khai; không có TAM/SAM/SOM đáng tin cậy cho niche Cellos. Đây là giới hạn kết luận, không phải khẳng định thị trường không tồn tại.

## Sources

[S1] Zephyr: [Introduction](https://docs.zephyrproject.org/latest/introduction/index.html), [releases/LTS](https://docs.zephyrproject.org/latest/releases/index.html), [safety scope](https://docs.zephyrproject.org/latest/safety/safety_overview.html). Primary technical documentation; không dùng làm market share.

[S2] FreeRTOS: [AWS overview](https://docs.aws.amazon.com/freertos/latest/userguide/what-is-freertos.html), [kernel MIT license](https://github.com/FreeRTOS/FreeRTOS-Kernel/blob/main/LICENSE.md). Primary technical/licensing.

[S3] Linux: [PREEMPT_RT mainline configuration](https://raw.githubusercontent.com/torvalds/linux/master/kernel/Kconfig.preempt), [RT behavior](https://raw.githubusercontent.com/torvalds/linux/master/Documentation/core-api/real-time/differences.rst), [hardware timing considerations](https://raw.githubusercontent.com/torvalds/linux/master/Documentation/core-api/real-time/hardware.rst). Primary source; mainline presence không là target qualification.

[S4] Commercial offerings: [QNX OS / OS for Safety](https://qnx.software/en/software/products-and-solutions/qnx-os-and-os-for-safety), [QNX lifecycle](https://www.qnx.com/support/knowledgebase.html?id=5015Y000001jIcx), [VxWorks Cert Edition](https://www.windriver.com/resource/vxworks-cert-edition-product-overview), [Wind River long-term support](https://www.windriver.com/resource/wind-river-long-term-support-services). Vendor descriptions of deliverables, không là adoption/certificate independent audit.

[S5] AMP substitutes: [Linux remoteproc](https://raw.githubusercontent.com/torvalds/linux/master/Documentation/staging/remoteproc.rst), [TI AM62x Linux/RTOS IPC](https://software-dl.ti.com/processor-sdk-linux/esd/AM62X/latest/exports/docs/linux/Foundational_Components_IPC62x.html). Framework và vendor reference, không suy ra hardware support cho Pi3.

[S6] Lifecycle: [systemd service supervision](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html#Restart=), [RAUC](https://rauc.readthedocs.io/en/latest/), [Mender state scripts](https://docs.mender.io/artifact-creation/state-scripts). Primary feature/limitation docs.

[S7] Rust precedents: [Hubris reference](https://hubris.oxide.computer/reference/), [Tock overview](https://tockos.org/documentation/getting-started/overview/).

[S8] [BlackBerry Q4/FY2026 financial results,09-04-2026](https://irp.cdn-website.com/586e2b1b/files/uploaded/Q4+FY26+Earnings+Press+Release+-+PDF.pdf). Sử dụng bảng doanh thu phân khúc cho FY ended28-02-2026; không dùng adoption PR, backlog hay forecast làm TAM.

[S9] [European Commission CRA policy and dates](https://digital-strategy.ec.europa.eu/en/policies/cyber-resilience-act), cập nhật27-07-2026. Primary regulatory summary; không phải tư vấn pháp lý hay khẳng định compliance cho Cellos.

[S10] Eclipse: [survey release announcement05-03-2026](https://newsroom.eclipse.org/news/announcements/eclipse-foundation-showcases-open-source-innovation-embedded-world-2026-releases), [download landing page](https://outreach.eclipse.foundation/2025-iot-embedded-developer-survey-report). Chỉ xác minh existence/topics; không có dữ liệu đủ để trích market-share percentages.

## Research Verification

Đã đọc source/roadmap/license và nguồn web nêu trên trực tiếp hoặc qua hai scout read-only. Không chạy benchmark, phỏng vấn khách hàng, QEMU mới, audit chứng nhận hay test sản phẩm. Phép quy đổi uptime đã được tính bằng Python. Các kết luận định vị/TCO là suy luận và giả thuyết được gắn nhãn; báo cáo này không thay đổi roadmap hay authorize một RTOS pivot.

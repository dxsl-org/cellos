# Cellos: tự động hóa lab, OS tự chủ và bầy đàn robot quân dụng

Ngày: 2026-09-05. Loại: strategic/architecture evaluation. Tích hợp thông tin người dùng bổ sung trong lúc nghiên cứu. Không thay roadmap, không triển khai code hay thiết kế hệ thống tác chiến.

## Verdict

**[INFERENCE — khuyến nghị cập nhật] Chọn thiết bị tự động hóa lab chuyên dụng làm sản phẩm kiểm chứng đầu tiên; định hướng Cellos là OS/runtime có thể kiểm chứng và tự chủ vận hành cho máy móc chuyên dụng. Không lấy hình dáng robot cá nhân, khả năng tháo lắp hay bầy đàn quân dụng tổng quát làm yêu cầu kiến trúc đầu tiên.** Đây là đề xuất thứ tự phát triển cần được chủ bài toán chấp thuận, không tự thay yêu cầu cuối của các đơn hàng.

- Cả ba còn trên giấy theo cập nhật của người dùng. Lab có job cụ thể hơn hai hướng còn lại, nhưng chưa có thiết bị hoặc workflow chi tiết để coi là product validation.
- Đơn OS tự chủ có thể là chương trình phát triển lõi dài hạn nếu được cấp nguồn lực và chia nghiệm thu phù hợp. Không dùng thành công robot làm bằng chứng thay thế được desktop/server.
- Đơn bầy đàn quân dụng phải được xét theo nhiệm vụ, quyền tự chủ, phạm vi tích hợp và trách nhiệm. Không mặc định quân dụng là có vũ trang; cũng không mặc định nó là logistics vô hại. Hiện chỉ phân tích chiến lược, rủi ro và vai trò cung cấp phần mềm chung, không đề xuất cơ chế phối hợp tác chiến hay sử dụng vũ khí.
- Người dùng phải cung cấp cả robot và phần mềm cho bài toán bầy đàn tổng quát. Giả thuyết trước về một fleet sẵn có để nhận gói OS nhỏ không còn là cơ sở xếp hạng hiện tại.
- Một đơn hàng có khách hàng trả cho năng lực tự chủ có thể tạo giá trị dù không rẻ hơn Linux. Phải so sánh trên toàn bộ requirement khách hàng, không dùng TCO để bác bỏ một yêu cầu độc lập-kernel thực sự trong hợp đồng.

Độ tin cậy: cao về current repo gaps và capability của các stack tham chiếu; trung bình về lựa chọn theo tổ chức hiện có; chưa đủ dữ liệu để xếp hạng doanh thu/lợi nhuận/xác suất nghiệm thu giữa các đơn. Không giả định ngân sách, thời hạn, quốc gia hay điều khoản IP.

## Scope Contract

### Dữ kiện từ người dùng

1. Robot dự kiến hai chân có bánh xe, có hai tay và gripper, tháo thành hai cấu hình; job lab là hỗ trợ nhà nghiên cứu làm việc chính xác cao, nguy hiểm hoặc lặp lại. Chưa chọn một quy trình và tiêu chí đo cụ thể.
2. OS tự chủ triển khai trên máy móc tổ chức, nhằm an toàn/bảo mật và không phụ thuộc bất kỳ bên nào; yêu cầu thay Windows/Linux trước đó chưa được rút lại. Cần chuyển mục tiêu độc lập tuyệt đối thành các nghĩa vụ có thể nghiệm thu.
3. Bầy đàn quân dụng là bài toán tổng quát; người dùng phải cung cấp cả robot và phần mềm, không chỉ kernel hay một component.
4. **Tất cả còn trên giấy.** Không giả định có module, fleet, integration platform hoặc production deployment sẵn có.

Chấp nhận các cơ hội/đơn hàng này theo thông tin người dùng; không tìm web để xác minh sự tồn tại. “Lưỡng dụng dân sự–quốc phòng” khác “hai cấu hình tháo/lắp”; phân tích cả khác biệt này mà không mặc định mọi cấu hình robot đều đáp ứng cả hai môi trường.

### Phạm vi và giới hạn

- Output: một thứ tự ưu tiên có điều kiện, vai trò Cellos trong từng cơ hội, ranh giới trách nhiệm, mốc nghiệm thu và dữ kiện có thể đảo quyết định.
- Acceptance: không đồng nhất kernel với toàn bộ robot, không đồng nhất tự chủ với tự viết mọi thứ, không đồng nhất nhiều robot với năng lực bầy đàn đã được chứng minh.
- Không thu nhỏ yêu cầu cuối của khách hàng một cách ngầm định. Mọi đề xuất pilot/appliance là giai đoạn/gói công việc cần khách hàng chấp thuận, không được gọi là đã giải xong yêu cầu thay Windows/Linux hoặc robot hoàn chỉnh.
- Không cung cấp kiến trúc điều phối tác chiến, phân công mục tiêu, dùng vũ khí, hay tối ưu khả năng tấn công của bầy đàn.
- Current Cellos: trusted Tier1 SAS/no_std; **Tier2 đã có substrate RV64/QEMU và bằng chứng cross-hart migration**, nhưng physical containment, DMA quarantine và production approvals còn mở; Tier3 Linux guest có giới hạn qualification; x86 chủ yếu QEMU; G3 external-gated; hai Pi3 B+ là development inventory. App guide ghi “not implemented” xung đột với `docs/project-roadmap.md:251-253`; báo cáo dùng trạng thái chi tiết trong roadmap, không suy thành production-ready. [R1–R4]

## Comparison Matrix

**[INFERENCE]** Đây là so sánh hướng tham gia, không phải đánh giá năng lực đội robot/nhà thầu chưa được mô tả.

| Hướng | Phạm vi có thể giới hạn để nghiệm thu | Giá trị có thể tạo cho Cellos | Rủi ro làm lệch roadmap | Vai trò nên chọn hiện tại |
|---|---|---|---|---|
| Thiết bị tự động hóa lab cố định, tác vụ chuyên dụng | Một workflow, một hardware profile, chất lượng thao tác và safety boundary rõ | Peripheral/service lifecycle, resource ownership, recovery, dữ liệu và vận hành tự chủ | Bị kéo sang lab automation tổng quát hoặc hình dáng humanoid trước khi biết task | Sản phẩm kiểm chứng ưu tiên |
| Robot cá nhân hoàn chỉnh, di động, tháo/lắp | Rộng hơn nhiều: base, tay, perception, safety, hai cấu hình độc lập | Deployment thực tế có chiều sâu | Phát triển OS và cả robot đồng thời | Đích mở rộng, không acceptance đầu |
| OS tự chủ cho function/hardware cohort cụ thể | Có thể giới hạn nếu hợp đồng cho phép | Source/build/update authority, platform qualification, tổ chức bảo trì | Pilot bị quảng bá thành thay toàn bộ Windows/Linux | Gói lõi/native platform có thể nhận |
| OS tự chủ thay đầy đủ desktop/server | Danh mục ứng dụng, thiết bị, quản trị và migration rất rộng | Có thể tài trợ tổ chức OS thực sự | Tất cả G2/G4/G5 mở cùng lúc; compatibility không có điểm dừng | Chương trình cấp tổ chức; không cam kết sole-prime theo năng lực hiện tại |
| Bầy đàn quân dụng tổng quát, cung cấp cả robot và phần mềm | Phạm vi end-to-end; nhiệm vụ cụ thể chưa chốt, tất cả trên giấy | Chưa chứng minh kernel là yếu tố quyết định | Phải xây robot, hệ thống và tổ chức assurance/support đồng thời | Không chọn làm chương trình sản phẩm đầu |

Không có cơ sở nói đơn quân sự chắc chắn ngân sách lớn hơn, lợi nhuận cao hơn, mua nhanh hơn hoặc cho phép bỏ qua chứng nhận. Hiện cũng không có fleet sẵn có tạo lợi thế tích hợp cho đơn này. Xếp hạng dựa trên khả năng định nghĩa một sản phẩm hẹp có giá trị, không dựa doanh thu hoặc độ sẵn sàng phần cứng chưa được cung cấp.

## Bài toán 1 — Thiết bị tự động hóa lab

### 1.1 Chọn một phần việc Cellos có thể sở hữu

**[INFERENCE]** Điểm vào phù hợp là quản lý cấu hình/phân hệ, trạng thái dịch vụ, quyền điều khiển ứng dụng và hồi phục có kiểm soát cho một module lab. Không đặt ngay Cellos vào mọi vòng điều khiển hoặc gánh toàn bộ stack robot.

Ranh giới đề xuất cho **robot lab không vũ trang, môi trường thử có kiểm soát**:

| Lớp | Chủ trách nhiệm đề xuất | Cellos không được tự nhận |
|---|---|---|
| Cơ khí/điện, motor/drive, cảm biến vị trí, bảo vệ và dừng khẩn | Đội cơ điện, drive/controller và thiết kế safety phù hợp | Một MCU thông thường hoặc một kernel Rust tự trở thành hệ thống an toàn đã được chứng nhận |
| Vòng điều khiển chuyển động theo phần cứng thực | Controller/RTOS/vendor stack được đo và thẩm định | Current Cellos đã đạt deadline hoặc cân bằng robot hai chân |
| Perception, planning, kinematics, camera/model SDK, công cụ robot | Linux/ROS/vendor stack khi đó là lựa chọn phù hợp | Native no_std đã thay được ROS, Python, GPU/NPU ecosystem |
| Candidate Cellos subsystem | Trusted module/service supervisor, audit/recovery và integration boundary hẹp | Tự quyết định rằng cấu hình cơ khí an toàn hoặc trực tiếp thay safety chain |

Linux có thể ở máy phát triển hoặc một compute module riêng trong bản thử; không bắt buộc đưa Linux vào VM của Cellos ngay. Tier3 là một phương án có qualification riêng, không phải prerequisite để chứng minh một module native.

ROS2 đã có managed lifecycle và ros2_control có hardware/controller lifecycle. Đây là đối chứng thật, không phải chỗ trống thị trường. Tài liệu ros2_control hiện còn cảnh báo command interfaces trong inactive state phụ thuộc implementation; lifecycle state không phải bằng chứng đã chặn chuyển động nguy hiểm. [S1–S3]

### 1.2 Tháo/lắp là đổi cấu hình vật lý, không chỉ hot-swap phần mềm

Không thể giữ process state rồi tự động tiếp tục chuyển động sau khi tháo/lắp. Cấu hình mới có thể thay đổi khối lượng, quán tính, mô hình động học, vùng va chạm, nguồn điện, cơ cấu đỡ, calibration và trạng thái dụng cụ.

Đối với lab evaluation, contract cần yêu cầu hệ thống về trạng thái được đánh giá an toàn, kiểm tra lại cấu hình và chỉ cho phép hoạt động theo quy trình xác nhận mới. Safety phải còn tác dụng nếu Cellos/Linux crash hoặc đang cập nhật. Với tải trọng/gravity, chỉ cắt torque/nguồn chưa chắc là trạng thái an toàn; cách xử lý thuộc thiết kế cơ điện và hazard assessment, không suy từ OS.

Tương tự, software checkpoint không biết chắc một thao tác vật lý đã xảy ra hay chưa. Sau lỗi trong một thí nghiệm, không được replay máy móc một bước cấp vật liệu/chuyển mẫu chỉ vì journal chưa ghi xong. Quy trình phải có cách đối soát kết quả vật lý và xử lý trạng thái chưa xác định; báo cáo này không thiết kế thao tác lab nguy hiểm.

### 1.3 Bắt đầu bằng cụm lab, nhưng không giả định “ba khớp” đủ cho mọi việc

Cần xác định một tác vụ với vật thể/fixture/vùng làm việc cụ thể trước khi chọn end-effector hoặc kernel features. Ba khớp và gripper không tự chứng minh khả năng định hướng dụng cụ, thao tác hai tay, tránh va chạm hoặc phục vụ mọi thiết bị lab. Có thể phải thay task/fixture/cơ khí, không phải sửa OS.

Điểm bắt đầu đề xuất: một thao tác lặp lại trên vật mẫu an toàn trong fixture cố định, không phải general lab assistant. Tách kiểm chứng cơ khí và chất lượng tác vụ khỏi kiểm chứng software lifecycle.

Ba nhu cầu cần tách thành ba acceptance: **lặp lại** (độ lặp lại, năng suất và lỗi task), **chính xác cao** (sai số của đại lượng thực cần đo/điều khiển và calibration), **nguy hiểm** (giảm phơi nhiễm, bảo vệ và quy trình xử lý lỗi được thẩm định). Position accuracy, repeatability và path accuracy không phải cùng một chỉ số; nguồn kỹ thuật RoboDK tách các phép đo này. Không lấy độ lặp lại do vendor công bố thay cho accuracy của toàn quy trình lab. [S10]

**[INFERENCE]** Bắt đầu bằng workflow lặp lại có vật mẫu thay thế an toàn; xác lập cách đo chất lượng trước, rồi mới thẩm định phiên bản cho môi trường nguy hiểm. Đây là thứ tự de-risk, không chứng nhận rằng thử vật mẫu an toàn đã đủ cho task nguy hiểm. Kernel determinism không tự bảo đảm độ chính xác cơ khí: cơ cấu, sensor, độ rơ/biến dạng, tool và calibration phải được đánh giá riêng.

Không chốt hai chân, hai tay hoặc tháo lắp trước khi biết workflow có cần chúng không. Cơ cấu cố định, một tay hoặc cơ cấu khác có thể phù hợp hơn; đây là phương án cần đánh giá chứ chưa lựa chọn hardware. Nếu hình dáng robot ban đầu là requirement bắt buộc, giữ nó thành đích riêng, không gọi workstation lab là đã hoàn thành robot đó.

Cũng cần làm rõ mô tả module: phần nào giữ hai tay, nguồn điện, controller, cảm biến, chân đế và safety controls khi tách? “Tách được” không đồng nghĩa “hai robot độc lập”. Hai cấu hình độc lập phải là requirement từ đầu, dù nghiệm thu theo từng bước.

### 1.4 Trình tự tạo sản phẩm

| Mốc đề xuất | Kết quả quan sát được | Không được suy ra |
|---|---|---|
| R0: phân rã và acceptance | Sơ đồ module/cơ điện, owner safety, một task, interface/controller hiện có | Chưa là demo có chuyển động hoặc production |
| R1: một module trên bàn thử | Bring-up exact board/interface, tác vụ được giới hạn, quan sát restart/abort đúng trong điều kiện an toàn | Chưa chứng minh robot lab đa năng |
| R2: một workflow lab được đối tác nghiệm thu | Chất lượng task, thao tác vận hành, phục hồi và support boundary có evidence | Chưa chứng minh base tự cân bằng/di động |
| R3: base và hai cấu hình độc lập | Từng cấu hình có chức năng, nguồn, điều khiển, safety và qualification riêng | Chưa suy rằng kết hợp chúng giữ nguyên evidence |
| R4: robot kết hợp và tháo/lắp | Nghiệm thu lại cấu hình kết hợp, ổn định/chuyển động và quy trình chuyển cấu hình | Không tự thành nền tảng bầy đàn hoặc OS desktop |

Ngưỡng deadline, jitter, dừng, lực/tải và task quality phải do phần cứng, control design, người dùng và safety owner xác lập. Đo worst-case/tail và failure path; không tự gán 1kHz hay số microsecond từ demo internet. ROS2 real-time guidance cũng nhấn mạnh deadline/jitter và nondeterminism, không chỉ mean latency. [S4]

### 1.5 Khi nào không dùng Cellos cho robot này?

- Linux/ROS + controller/RTOS hiện có đáp ứng cùng contract với chi phí tổng thấp hơn và không thiếu một yêu cầu tự chủ mà Cellos giải được.
- Cellos chỉ thêm một máy tính, một đường truyền và một failure mode mà không giảm pain của integrator.
- Pain chính là cơ khí, cân bằng, perception hoặc safety certification, không phải module/service lifecycle.
- Tháo/lắp hiếm và một quy trình shutdown/recommissioning đã đủ: không tạo distributed hot-swap framework chỉ để tránh thao tác này.
- Lịch triển khai đòi production trước khi exact hardware, safety và các gate Cellos tương ứng có thể được đáp ứng.

## Bài toán 2 — OS tự chủ thay Windows và Linux

### 2.1 Hiểu nghiêm túc yêu cầu “thay thế”

Nếu yêu cầu theo nghĩa đen là **không dùng kernel Windows/Linux và thay các workload hiện có**, không thể nộp một Linux distribution hay một Linux guest rồi gọi là hoàn thành. Phải tách đích cuối khỏi mốc phát triển đầu, và chỉ thay phạm vi khi bên đặt hàng đồng ý.

Ba nhóm acceptance thường bị gộp:

| Nhóm | Nội dung cần nghiệm thu | Ý nghĩa với Cellos |
|---|---|---|
| Tự chủ vận hành và nguồn cung | Quyền kiểm tra/sửa mã, build, ký, cập nhật, giữ dữ liệu/khóa, incident response, chuyển nhà cung cấp | Không bắt buộc phải viết kernel mới nếu hợp đồng cho phép OSS upstream |
| Kernel độc lập trên một tập thiết bị/workload | Native kernel không Linux/Windows trong phạm vi được chỉ định; hardware/driver/runtime và maintenance được sở hữu/quản lý | Có thể là chương trình Cellos có gói nghiệm thu rõ |
| Thay đầy đủ workstation/server estate | Toàn bộ application/device/enterprise workflow và service operations tương ứng | Là chương trình hệ sinh thái và migration, không phải chỉ mở lane G2 |

Schleswig-Holstein là một đối chứng hữu ích về cách phân kỳ: thông báo chính thức tháng10-2025 tách mail migration đã hoàn thành, office/collaboration và Linux thử nghiệm thành các bước riêng. Đây là ví dụ chủ quyền số có dùng Linux, **không** chứng minh đáp ứng một hợp đồng cấm Linux. Không suy tình trạng Linux pilot tháng10-2025 thành tình trạng mới nhất tháng09-2026. [S6]

### 2.2 Cellos có thể tham gia như thế nào?

**[INFERENCE] Có thể làm lõi của chương trình độc lập-kernel, hoặc chịu trách nhiệm một platform/workload package dưới nhà tích hợp chính.** Gói đầu có thể là một native appliance hoặc cohort phần cứng cố định với ứng dụng được nêu tên, nếu nằm trong lộ trình được bên đặt hàng chấp thuận.

Nếu acceptance đầu đã đòi dùng thay PC Windows/Linux hàng ngày trên phần cứng đa dạng, current Cellos chưa là nền tảng đủ để cam kết giao toàn gói. X86 QEMU/ViUI không chứng minh browser/office compatibility; Tier1 trusted SAS không phải boundary cho ứng dụng bất kỳ; Tier2 và native Rust std vẫn có gap. [R1–R3]

Phải kiểm kê ít nhất:
- Ứng dụng nghiệp vụ, browser, office/document fidelity, macro, chữ ký số, mail/collaboration.
- Driver/GPU, mạng, máy in/scanner, USB/token/smartcard, font/input method, accessibility.
- Identity/PKI, cấu hình endpoint, patching, logging, backup/recovery và helpdesk.
- Firmware, compiler/dependency supply chain, source/redistribution rights, build/sign/update authority, SBOM và security response.
- Exact devices, security/threat model, accreditation/certification nào thực sự được yêu cầu, và ai có quyền ký nghiệm thu.

Nguồn như NIST SSDF cung cấp vocabulary cho secure development và acquisition, không cấp chứng nhận cho Cellos và không tự trở thành luật của quốc gia đặt hàng. Chiến lược OSS của EC là ví dụ lịch sử về governance, kỹ năng và procurement/TCO, không dùng làm current adoption statistic. [S7–S8]

### 2.3 Kinh phí thay đổi tổ chức, không thay thế bằng chứng

Không nên bác bỏ cơ hội chỉ vì hiện Cellos có một maintainer: một chương trình được tài trợ có thể tạo đội BSP/driver, runtime/apps, security/release, testing và migration/support. Nhưng phải phân bổ trách nhiệm và deliverable rõ trước khi mở rộng cam kết. Subagents không thay cho independent accountable approvers. [R5]

Các điều khoản nền tảng cần chốt: background IP của Cellos, quyền với phần phát triển từ đơn hàng, quyền tái sử dụng cho robot, điều kiện công bố upstream, custody khóa, hỗ trợ sau bàn giao, quản lý lỗ hổng và điều kiện chấm dứt/chuyển giao.

Tier3 Linux có thể là migration bridge **nếu hợp đồng cho phép**; nó không loại bỏ Linux, không chứng minh Windows guest compatibility và không tự giải licensing. Không được hứa full Windows compatibility chỉ vì có hypervisor.

### 2.4 Chuyển “không phụ thuộc bất kỳ bên nào” thành nghiệm thu

**[INFERENCE]** Độc lập tuyệt đối khỏi mọi bên không phải cam kết khả thi cho một OS triển khai trên máy móc hiện đại: vẫn có phần cứng, firmware, công cụ build, thư viện và người bảo trì. Mục tiêu có thể kiểm chứng hơn là **không bên ngoài nào có quyền đơn phương cắt khả năng vận hành hoặc ngăn tổ chức tiếp tục bảo trì/chuyển nhà cung cấp** trong phạm vi đã xác định.

| Mặt tự chủ | Bằng chứng đề xuất |
|---|---|
| Vận hành | Chức năng được chỉ định hoạt động khi không truy cập dịch vụ cấp phép/cloud ngoài; ngoại lệ được khai báo |
| Dữ liệu và khóa | Tổ chức kiểm soát lưu trữ, xuất dữ liệu, khóa và quyền quản trị; không phụ thuộc tài khoản do vendor độc quyền nắm |
| Mã nguồn và build | Quyền sử dụng/sửa/phân phối phù hợp, dependency inventory và khả năng rebuild từ vật liệu bàn giao |
| Cập nhật và ứng phó lỗi | Tổ chức có thẩm quyền phát hành/thu hồi/cập nhật và đội thực sự thực hiện được, không chỉ có source archive |
| Chuyển nhà cung cấp | Một đội độc lập tiếp nhận, vận hành, sửa và phát hành được theo bộ hồ sơ bàn giao |
| Phần cứng/firmware | Mọi phụ thuộc không thay thế được được ghi rõ cùng rủi ro, hỗ trợ và phương án chuyển đổi; không hứa tự chủ silicon vì có kernel riêng |

Mã nguồn mở không đồng nghĩa không phụ thuộc, nhưng có thể cho quyền kiểm soát và chuyển nhà cung cấp; tự viết kernel cũng không tự loại vendor lock-in. Cellos do một người nắm toàn bộ kiến thức có thể tạo một nút phụ thuộc mới nếu không có handover và maintenance organization. GDS khuyến nghị đánh giá support, khả năng bảo trì, licence và cả exit/transition costs cho cả OSS lẫn proprietary software. [S11]

Chạy offline không tự đồng nghĩa bảo mật hoặc safety. Ba bộ tiêu chí — quyền tự chủ, chống truy cập/tác động trái phép, và không gây hại vật lý — cần evidence riêng. Nếu hợp đồng thực sự cấm Linux/Windows, vẫn phải đáp ứng điều đó; không dùng diễn giải sovereignty để lách yêu cầu.

## Bài toán 3 — Bầy đàn robot quân dụng

### 3.1 Không chọn chỉ vì có nhiều robot hoặc đơn hàng quốc phòng

**[INFERENCE]** Đây là một bài toán hệ thống-of-systems, trong khi current Cellos mới có evidence hẹp theo node/platform. Khả năng IPC hoặc local C2C không tự chứng minh nhiều robot vận hành đúng như một hệ thống. Repo còn ghi rõ local/single-guest evidence không đóng two-node/remote/public/production requirements. [R2]

Ở mức chiến lược, thêm nhiều robot kéo theo thêm tương tác cần nghiệm thu, trách nhiệm vận hành và cấu hình ngoài kernel. Không tự giả định phải dùng mesh hay chi phí luôn tăng N²; kiến trúc và nhiệm vụ chưa có. Không có cơ sở kết luận OS mới là phần thiếu của đơn hàng.

### 3.2 Phân loại nhiệm vụ trước khi định vị Cellos

| Loại đơn hàng | Có thể đánh giá/đề xuất ở đây | Quyết định chiến lược |
|---|---|---|
| Đội robot phi tác chiến: hỗ trợ lab, logistics được xác định, cứu hộ hoặc kiểm tra kỹ thuật không phục vụ sử dụng vũ lực | Phạm vi sản phẩm, tích hợp phần mềm chung, safety/operational acceptance, support/IP | Có thể xét gói độc lập, sau khi end-use và trách nhiệm được xác nhận |
| Nhiệm vụ quân dụng chưa rõ | Commercial terms, integration ownership, mức tự chủ và yêu cầu con người giám sát | Chưa có đủ cơ sở chọn làm định vị chính |
| Phối hợp tác chiến hoặc chức năng vũ khí | Chỉ governance/pháp lý, trách nhiệm, rủi ro và human control ở mức khái quát | Không cung cấp thiết kế phối hợp, lựa chọn/đánh mục tiêu hay tối ưu năng lực tấn công |

Không gọi “recon/targeting support” là phi tác chiến chỉ bằng cách bỏ từ vũ khí khỏi tên module. Cần xét chức năng thực và end-use, không chỉ nhãn thương mại.

Lời kêu gọi chung UN–ICRC ngày25-08-2026 nhấn mạnh sự suy giảm kiểm soát con người, trách nhiệm và nguy cơ với dân thường của autonomous weapon systems, đồng thời thúc đẩy quy tắc ràng buộc mới. Đây là nguồn về rủi ro/quản trị, không phải bằng chứng đã có lệnh cấm chung cho mọi robot quân dụng hoặc một luật tự động áp dụng cho đơn hàng của người dùng. [S9]

### 3.3 Vì sao không chọn full swarm là hướng đầu?

- Đã xác nhận scope là **cả robot và phần mềm**, không phải một gói OS nhỏ. Đây là một chương trình robotics end-to-end, vượt phạm vi một roadmap kernel; không còn giả thuyết có thể nhận fleet sẵn có.
- Năng lực hệ thống, assurance và điều kiện thử thuộc nhiều bên; phát triển kernel không thay thế chúng.
- Quyền IP, giới hạn công bố, export/end-use restrictions và khả năng tái sử dụng cần kiểm tra theo hợp đồng/quốc gia; không mặc định được hay không được.
- Một khách hàng/đơn hàng đơn lẻ có thể kéo toàn roadmap sang đặc thù của họ; chưa có bằng chứng thị trường tiếp nối.
- Robot và phần mềm đều còn trên giấy. Đội chuyên môn, facilities, qualification và năng lực hỗ trợ chưa được mô tả; không suy từ hai Pi3 thành một nền tảng bầy đàn.

### 3.4 Điều kiện để xem xét lại quyết định chiến lược

Với scope hiện tại, không chọn chương trình này làm đường triển khai đầu của Cellos. Chỉ xem xét lại khi có nhiệm vụ được giới hạn, tổ chức end-to-end, nguồn lực, trách nhiệm nghiệm thu và khung pháp lý/safety rõ ràng. Nếu các bên muốn chuyển sang một gói phần mềm phi tác chiến hẹp thì phải sửa scope một cách minh bạch; đó không phải đơn hàng đang được mô tả.

Không lập kiến trúc hoặc roadmap phối hợp tác chiến/vũ khí trong phân tích này.

## Ranked Recommendation — Một chương trình chính, không ba backlog

### Phương án A — Tự động hóa lab dẫn đường (ưu tiên hiện tại)

- Chọn một workflow lab và chỉ số chất lượng cần đo trước; chưa chốt humanoid, hai chân hoặc cơ cấu tháo lắp làm hình dạng sản phẩm đầu.
- Cellos sở hữu một phần runtime/lifecycle/platform hẹp, còn đội cơ điện/robotics sở hữu các lớp tương ứng.
- Đơn OS tự chủ được chuẩn bị như một gói core/platform hoặc chương trình có tổ chức riêng; tái dùng evidence đúng lớp.
- Không mở bầy đàn thành chương trình sản phẩm song song.

Cả ba còn trên giấy, nên bước đầu là định nghĩa workflow lab có thể nghiệm thu và phương án cơ điện/safety, không phải port OS lên một robot đã có. Ưu tiên lab là lựa chọn về phạm vi sản phẩm; chưa chứng minh ngân sách, thời hạn hay tốc độ giao hàng tốt hơn.

### Phương án B — Đơn hàng được tài trợ và giới hạn rõ dẫn đường

Có thể chọn gói OS tự chủ cho một cohort máy móc trước nếu có scope native/kernel độc lập rõ, nguồn lực và tổ chức qualification được bố trí. Hiện chưa có dữ kiện để ưu tiên theo ngân sách. Điều kiện “fleet phi tác chiến sẵn có” trong bản phân tích trước không còn áp dụng cho đơn bầy đàn tổng quát do người dùng phải cung cấp cả robot và phần mềm.

Phương án này không yêu cầu một demo robot phải đứng trước mọi công việc chính phủ. Robot success cũng không là prerequisite kỹ thuật cho desktop/server. Chúng chỉ có thể chia sẻ một số nền tảng, không phải cùng product contract.

### Phương án C — Full robot cá nhân + full swarm + full OS thay Windows/Linux

**Không khuyến nghị với tổ chức hiện tại.** Ba chương trình tạo ba nhóm nghĩa vụ phần cứng, ứng dụng, system assurance và support. Muốn làm đồng thời phải có đội và ngân sách độc lập, owner nghiệm thu độc lập, cùng governance core; không chỉ chia todo cho một maintainer.

## Roadmap Implications

| Nhóm | Quyết định đề xuất |
|---|---|
| Kernel correctness/ownership/lifecycle | Giữ supported-path fixes; mở thêm theo acceptance của chương trình chính |
| Robot integration | Chọn đúng board/interface/module cần; sensor lane hiện deferred chỉ được mở lại đúng scope |
| Hot-swap | Dùng cho service/update contract được phép; không hứa hot-swap motion/safety hoặc physical-state rollback |
| C2C | Không biến thành lý do mở distributed platform ngay; một proof local không đạt two-node/remote qualification |
| G2 x86/desktop | Chỉ ưu tiên khi gói OS tự chủ được chọn cần exact platform/workload; không gắn thêm vào robot |
| G3 NPU | Model/vendor need có thật mới mở, với target/SDK/licence gates giữ nguyên; cả robot và đơn quân dụng đều không tự gỡ gate |
| G4/G5/runtime breadth | Theo ứng dụng và trust boundary đã chọn; không blanket prerequisite cho mọi demo |
| Production/security | Gate repo giữ fail-closed; government/robot requirements phải map riêng, không coi ADR nội bộ là đủ chứng nhận bên ngoài |

Quy tắc WIP: một deliverable sản phẩm chính cộng supported-path maintenance. Phần dùng chung chỉ được tái sử dụng theo IP/security terms và evidence tương ứng; không dùng tiền, key, dữ liệu hay hồ sơ nghiệm thu giữa các hợp đồng tùy ý.

Báo cáo trước gợi ý tìm nhiều buyer như một discovery gate cho thị trường giả định. Với cơ hội/đơn hàng hiện có, không áp ngưỡng đó máy móc. Gate hữu ích hơn là người nghiệm thu, use case, acceptance, nguồn lực, IP và phạm vi trách nhiệm cụ thể của chính đơn này.

## Common Pitfalls and Stop Conditions

1. **Robot hai hình dạng = hai sản phẩm đã sẵn sàng:** sai; mỗi cấu hình cần kiểm chứng cơ khí, nguồn, safety và task riêng.
2. **Robot cá nhân = consumer mass market:** chưa có dữ kiện. Đầu tiên nên bán một kết quả chuyên dụng cho đối tác, không hứa assistant đa năng.
3. **OS tự chủ = Linux distro là đủ:** sai nếu hợp đồng cấm Linux; cũng sai theo chiều ngược lại rằng mọi sovereignty requirement đều cấm OSS upstream.
4. **Rust/capability = certification:** sai; current SAS trust boundary, boot/hardware, driver và whole-system properties cần evidence riêng.
5. **Đơn quốc phòng = nên ưu tiên bằng mọi giá:** chưa có dữ kiện về profitability, accountability hoặc khả năng giao.
6. **Bầy đàn là nhiều instance của cùng OS:** chưa đủ; vận hành và nghiệm thu hệ thống không được suy từ per-node runtime evidence.
7. **Pilot = production:** sai. Pi3/QEMU hiện không đóng production root/admission/qualified hardware gates. [R1–R4]
8. **Khách cần feature = Cellos phải viết lại stack:** không; dùng vendor/ROS/RTOS hoặc adapter nếu đáp ứng hợp đồng tốt hơn và không vi phạm yêu cầu tự chủ.

Dừng mở rộng Cellos cho một gói nếu kernel không giải requirement nào mà baseline thiếu, không kiểm soát được acceptance, không có đường qualification hoặc scope bắt buộc vượt trust/runtime support hiện tại mà không có chương trình tài trợ để đóng gap.

## Active Refutation

- **“Mô-đun robot cần OS mới để có lifecycle” — CONTESTED.** ROS2/ros2_control đã có primitives tương ứng. Phải chứng minh giá trị thêm của Cellos, không lấy tên capability làm USP. [S1–S3]
- **“Tự chủ phải bỏ Linux trong mọi trường hợp” — CONTESTED.** Có government migration dùng Linux/OSS cho sovereignty. Nhưng điều đó không sửa yêu cầu cấm Linux nếu khách hàng đã đặt ra. [S6–S7]
- **“Có bầy đàn và có C2C nghĩa là Cellos phù hợp ngay” — UNVERIFIED.** Repo ghi rõ ceiling local/QEMU; nhiệm vụ, platform và hợp đồng bầy đàn chưa được cung cấp. Không có benchmark hay field evidence cho kết luận này. [R2]

## Unresolved Questions — Dữ kiện có thể đảo quyết định

Không cần hỏi lại thông tin repo cung cấp. Cần từ chủ bài toán/đơn hàng:

1. **Lab:** workflow đầu tiên là gì; “chính xác” là đại lượng nào, tolerance và phương pháp đo nào; ai chịu cơ điện/safety; morphology ban đầu có bắt buộc không? Đã biết mọi thứ còn trên giấy, không hỏi lại tình trạng prototype.
2. **OS tổ chức:** cohort máy móc/ứng dụng nào trước, yêu cầu độc lập-kernel có bắt buộc không, và bên nào nghiệm thu các quyền vận hành/build/keys/update/handover? Đã biết mục tiêu là safety/security và tự chủ.
3. **Bầy đàn:** chỉ cần mô tả end-use không nhạy cảm và phạm vi governance để đánh giá chiến lược. Đã biết scope gồm cả robot/phần mềm và chưa có hệ thống sẵn; không hỏi lại hoặc giả định đây là gói kernel riêng.
4. **Cả ba:** ai nghiệm thu, mốc bắt buộc, phạm vi quyền sửa stack, điều khoản IP/reuse, support và nguồn lực cho exact hardware/qualification?

Không yêu cầu dữ liệu tác chiến hoặc thông tin mật để lựa chọn chiến lược. Một bản mô tả không nhạy cảm về phạm vi và acceptance là đủ cho bước tiếp theo.

## Resources & References

### Repository
- **[R1]** `docs/roadmap/product-stages.md`: G overlays, inventory/evidence, G1/G2/G3 và G4/G5 boundaries.
- **[R2]** `docs/project-roadmap.md`, `docs/roadmap/current-focus.md`, `docs/roadmap/open-risk-register.md`: sensor deferred, local C2C ceiling, exact-device/production gaps.
- **[R3]** `docs/app-development-guide.md:21-73` cho Tier1/no_std và Tier3; **trạng thái Tier2 lấy từ `docs/project-roadmap.md:251-253`**: substrate RV64/QEMU và cross-hart migration đã có, physical containment/DMA quarantine/production approvals còn mở. App guide có mô tả Tier2 lỗi thời/xung đột; báo cáo đã sửa nhận định “unimplemented”, không sửa canonical docs trong nghiên cứu này.
- **[R4]** `docs/decisions/0006-block-production-root-pending-exact-product-evidence.md`: chưa chọn exact production root; development không được nâng thành production claim.
- **[R5]** `docs/decisions/0013-solo-first-development-independent-promotion.md`: solo development khác independent accountable approval.
- **[R6]** [Báo cáo portfolio trước](research-260905-1246-cellos-portfolio-focus.md); [plan SAS/LBI](../260905-1139-sas-lbi-outcome-closure/plan.md). Kết luận workload thị trường giả định được cập nhật bởi thông tin mới; không sửa plan tự động.

### Primary sources
- **[S1]** [ROS2 managed nodes](https://design.ros2.org/articles/node_lifecycle.html). Foundational design, cập nhật2021; không coi như safety standard.
- **[S2]** [ros2_control hardware component lifecycle, Kilted](https://control.ros.org/kilted/doc/ros2_control/hardware_interface/doc/lifecycle_of_a_hardware_component.html). Tài liệu đọc ghi Sep2026; có warning cụ thể về inactive command interfaces.
- **[S3]** [ros2_control Controller Manager, Kilted](https://control.ros.org/kilted/doc/ros2_control/controller_manager/doc/userdoc.html); [micro-ROS Agent](https://github.com/micro-ROS/micro-ROS-Agent/blob/kilted/README.md). Nguồn integration, không chứng minh safety-certified deployment.
- **[S4]** [ROS2 real-time programming, Jazzy](https://docs.ros.org/en/jazzy/Tutorials/Demos/Real-Time-Programming.html). Release được hỗ trợ, không gọi là latest; không lấy benchmark demo làm số đo robot/Cellos.
- **[S5]** [ISO13482:2014 public abstract](https://www.iso.org/standard/53820.html). Intended-use/exclusions cho personal-care robots; chưa chọn đường compliance cho robot lab hoặc quân dụng từ một mô tả ngắn.
- **[S6]** [Schleswig-Holstein, thông báo chính thức06-10-2025](https://www.schleswig-holstein.de/DE/landesregierung/ministerien-behoerden/I/Presse/PI/2025/cds/251006_cds_ox). Source cho phased migration, không cho latest estate-wide completion hay yêu cầu của khách hàng người dùng.
- **[S7]** [European Commission OSS strategy](https://commission.europa.eu/about/departments-and-executive-agencies/digital-services/open-source-software-strategy_en). Trang chứa chiến lược2020–2023 và2014–2017; chỉ dùng làm historical governance/procurement example.
- **[S8]** [NIST SP800-218 SSDF1.1](https://csrc.nist.gov/pubs/sp/800/218/final), February2022. Procurement/supplier-development vocabulary; không phải product certification hoặc luật mặc định áp dụng.
- **[S9]** [UN–ICRC renewed call,25-08-2026](https://www.icrc.org/en/statement/renewed-call-un-secretary-general-and-icrc-president-adopt-rules-autonomous-weapons). Human control, accountability và nguy cơ với civilians; không đồng nhất mọi military robot với AWS.
- **[S10]** [RoboDK ISO9283 performance testing](https://robodk.com/doc/en/Robot-Validation-ISO9283.html): technical documentation phân biệt accuracy, repeatability, path accuracy và calibration. Không dùng vendor marketing làm market evidence hoặc nguồn này thay toàn văn tiêu chuẩn.
- **[S11]** [UK GDS — Be open and use open source](https://www.gov.uk/guidance/be-open-and-use-open-source): support, maintenance, interoperability, licence và exit/transition costs. Nội dung ghi cập nhật2021, dùng như procurement guidance lịch sử, không gọi là chứng nhận Cellos hoặc luật áp dụng cho tổ chức người dùng.

## Evidence and Deliverable Limits

Hai scout read-only nghiên cứu robot và sovereign programme; Main kiểm tra source/repo, tích hợp đơn bầy đàn mới ở mức chiến lược và quản trị. Đã đọc nguồn ROS, government migration, NIST và UN–ICRC. DoD3000.09 PDF bị403 ở hai official endpoints; browser fallback không có nội dung đọc được, nên không dùng kết quả search như bằng chứng đã đọc directive. Không có kiểm thử robot, benchmark Cellos, thẩm định pháp lý, procurement acceptance hay production validation trong nghiên cứu này. Chỉ tạo báo cáo; không đổi code, roadmap, contract hay security gates.

# Biên bản trao đổi và research — 2026-10-19

## 1. Ý định người dùng và trạng thái quyết định

Người dùng nhận thấy kiến trúc filesystem BeOS/Haiku thú vị và muốn biết có phù hợp Cellos,
vì AI agent sẽ tìm file/content nhiều và nhanh. Cuộc trao đổi mở rộng sang VFS vs Search Cell,
chi phí index, NTFS/Linux, native disk format và giá trị tham khảo của filesystem hiện đại.
Cuối phiên, người dùng yêu cầu **lưu plan dang dở ở analysis/research để suy nghĩ và quyết định sau**.

Không có quyết định chọn BFS, RedoxFS, engine index, metadata schema, native format mới,
consistency model hoặc triển khai Search Cell. Nhận định “đo bằng Cell riêng trước” là hướng
đã thảo luận thuận lợi, không được nâng thành implementation approval.

## 2. Phân biệt ba tầng và bốn loại index

- **VFS:** API/namespace, dispatch backend, authority và lifecycle; không có một disk format riêng.
- **CellosFS/BFS/RedoxFS:** filesystem implementation và on-disk representation.
- **Search/Index Cell:** dịch vụ có thể đọc nguồn và duy trì catalog/index dẫn xuất.

| Index | Trả lời | Không tự cung cấp |
|---|---|---|
| Directory/name lookup | Tên trong directory trỏ tới object nào? | Full-volume attribute/content query |
| Secondary attribute index | File nào có thuộc tính bằng/trong khoảng giá trị? | Tìm đoạn giải thích trong PDF |
| Full-text/inverted index | Từ/cụm từ xuất hiện ở file/đoạn nào? | Semantic similarity hoặc arbitrary regex luôn nhanh |
| Semantic/vector index | Đoạn nào gần nghĩa câu hỏi? | Exact symbol/path match đáng tin thay text search |

HTree/B-tree trong ext4/Btrfs/NTFS không đồng nghĩa có general full-text search.
Windows Search duy trì value/inverted indexes ngoài NTFS, dùng USN journal để cập nhật incremental.
BFS tích hợp indexed attributes và live queries; query được lưu là điều kiện, không phải danh sách tĩnh.

## 3. VFS hay Cell riêng: điều đã trao đổi

**Hướng đề xuất, chưa chốt:** nguồn dữ liệu ở filesystem; content index ở Cell riêng;
chỉ bổ sung primitive VFS khi requirement/số đo chứng minh thiếu.

- Contract file identity/lifetime, revision/read consistency, generic metadata và changes cần liên quan VFS/backend.
- Tokenization, PDF/OCR, ranking, embeddings và index rebuild policy không mặc nhiên thuộc VFS.
- Có `query` API ở VFS vẫn có thể delegate engine; đây là quyết định API riêng.
- Đưa engine vào VFS không tự nhanh hơn; tách Cell không tự rẻ hơn hay được MMU isolation.
- Native index có thể cần nếu yêu cầu update/query atomic hoặc read-after-write tuyệt đối;
  phải thiết kế transaction/recovery, không suy ra từ vị trí code.
- Live query membership có thể nằm ở Search Cell trên change feed; không cần đưa query evaluator vào filesystem.
- Change notifications không phải durable log; phải xử lý gap/overflow/restart và reconciliation.
- Snapshot/versioning hữu ích tiềm năng nhưng không phải scope đã yêu cầu triển khai.

## 4. Chi phí và lợi ích indexing

Index vẫn tốn đọc/parse/tokenize, CPU, RAM, disk space, write I/O, update/merge/recovery ở mọi vị trí.
Sync index có thể kéo dài đường ghi; async cho batching/coalescing/quota nhưng trả kết quả có thể cũ.
Không coi “để trong Cell” là tối ưu tài nguyên nếu chưa đo cùng freshness/semantics.

Mô hình định hướng: scan tốn `Q*S`; index tốn `B+U+Q*L`, với Q là số query,
S chi phí scan/query, B build/rebuild, U duy trì, L indexed lookup/query.
Index có thể lợi khi `Q*(S-L) > B+U`; đây không phải benchmark và chưa tính hết memory/latency.
Nhiều agent dùng một shared index có thể tránh crawl trùng; mức lợi chưa đo.
Corpus nhỏ/ít query/file tạm ghi-xóa nhiều có thể hợp scan hơn.

## 5. So sánh nguồn kiến trúc để tham khảo

| FS | Cơ chế đáng chú ý | Phần đáng học cho Cellos | Giới hạn suy luận |
|---|---|---|---|
| BFS (BeOS/Haiku) | Extents/block runs, directory/attribute B+trees, typed attrs, small-data area, metadata journal | Metadata là first-class, selective indexes, query/live query | Không dùng làm chuẩn duy nhất về integrity, snapshots hay NVMe scalability |
| RedoxFS | Rust, CoW, data/metadata checksums, transparent encryption, microkernel adaptation | Implementation và service integration gần Cellos hơn | ZFS-inspired không có nghĩa đủ tính năng/qualification của ZFS |
| ext4 | Extents, HTree, JBD2 journal, metadata checksums | Durability/ordering, tooling, format evolution | Metadata checksum không mặc định bảo vệ toàn bộ regular file data |
| XFS | B+trees, allocation groups, extent management, metadata log | Scale inode/directory, concurrency, reservation | Không đánh đồng reflink với full filesystem snapshots hoặc data checksums |
| Btrfs | CoW trees, shared extents, subvolumes/snapshots, checksums | Generation, versioning, scrub, extent references | Feature/configuration có maturity khác nhau; không blanket production label |
| OpenZFS | CoW transaction groups, storage pool, end-to-end checksums, snapshots, scrub/repair | Integrity/publication và lifecycle vận hành | Không bê toàn stack vào Cellos; repair cần nguồn đúng/redundancy |

Không có xếp hạng benchmark trong bảng. CoW không tự chứng minh crash consistency/snapshots;
checksum không tự tạo repair; Rust không loại lỗi publication, flush hoặc allocator reuse.
BFS vẫn là reference tốt về metadata/query. Modern FS cung cấp chuẩn đối chiếu recovery/scalability.
Không đề xuất ghép tất cả feature hoặc trộn nửa journal/nửa CoW thiếu một recovery model thống nhất.

## 6. Điều chỉnh nhận định trong hội thoại

1. Ban đầu nghiêng về giữ CellosFS và tách Search Cell để đo.
2. Đọc format cụ thể phát hiện limits 7 entry/directory, 46 byte/name và 256 inode slots ở native engine.
3. Vì vậy phải tách bài toán storage capacity/correctness khỏi search efficiency; index không chữa capacity.
4. Chưa kết luận thay backend. Nâng native format và chọn backend khác vẫn là các phương án mở.
5. Không suy ra feature tồn tại chỉ vì ADR/comment dùng từ “CoW”, “extent B-tree” hoặc “crash-resilient”.
6. Article Haiku năm 2007 về unlink/open và chkbfs là bài học lịch sử, không chứng minh bug còn nguyên hôm nay.
7. Chưa benchmark, chưa reproduce native limits bằng runtime, chưa qualification engine/FS/hardware.

## 7. Nguồn ngoài và phạm vi bằng chứng

Nguồn được đọc/tìm trong cuộc trao đổi; không pin upstream revision hay coi feature docs là test report.
Không chọn nhầm Linux `bfs` (SCO boot filesystem) thành Be File System; tên module Linux liên quan là `befs`.

- [BFS design book — Dominic Giampaolo](https://www.haiku-os.org/legacy-docs/practical-file-system-design.pdf): cấu trúc, attributes/indexes, journal; nguồn lịch sử của tác giả.
- [Haiku queries](https://www.haiku-os.org/docs/userguide/en/queries.html): live và saved queries.
- [Haiku index management](https://www.haiku-os.org/docs/userguide/en/index.html): per-volume index, selective attributes và reindex dữ liệu cũ.
- [Haiku full-text project, 2009](https://www.haiku-os.org/blog/general_maximus/2009-05-17_full_text_searching_and_indexing): minh họa content indexing tách biệt, không khẳng định trạng thái hiện nay.
- [Why BFS needs chkbfs, 2007](https://www.haiku-os.org/blog/axeld/2007-10-05_why_bfs_needs_chkbfs): giới hạn recovery lịch sử.
- [RedoxFS official book](https://doc.redox-os.org/book/redoxfs.html): feature list hiện công bố; grow/shrink có experimental warning.
- [RedoxFS upstream mirror](https://github.com/redox-os/redoxfs): không đồng nhất với vendored revision của Cellos.
- [Windows Search indexing process](https://learn.microsoft.com/en-us/windows/win32/search/-search-indexing-process-overview): gatherer, value/inverted index, USN notifications và recovery crawl.
- [ext4 directory structures](https://www.kernel.org/doc/html/v6.17/filesystems/ext4/directory.html): directory lookup; lần fetch trong phiên chỉ trả metadata, không coi full body đã đọc.
- [ext4 journal](https://docs.kernel.org/filesystems/ext4/journal.html): metadata consistency và journal/data mode.
- [ext4 checksums](https://www.kernel.org/doc/html/v6.18/filesystems/ext4/checksums.html): metadata checksum.
- [XFS documentation](https://www.kernel.org/doc/html/latest/filesystems/xfs/index.html): logging và filesystem design.
- [XFS self-describing metadata](https://docs.kernel.org/filesystems/xfs/xfs-self-describing-metadata.html): validation/scalability.
- [RHEL filesystem guidance](https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/9/html/managing_file_systems/overview-of-available-file-systems_managing-file-systems): ví dụ deployment XFS, không benchmark Cellos.
- [Btrfs design](https://btrfs.readthedocs.io/en/stable/dev/dev-btrfs-design.html): trees, directory indexes, generations và extent references.
- [Btrfs checksumming](https://btrfs.readthedocs.io/en/latest/Checksumming.html): data/metadata checksums mặc định; kiểm tra exceptions/config khi chọn.
- [Btrfs feature status](https://btrfs.readthedocs.io/en/latest/Status.html): đánh giá từng feature, không blanket label.
- [OpenZFS CoW](https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html): CoW/snapshot architecture.
- [OpenZFS checksums](https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Data%20Storage/Checksums.html): detect, redundant repair và scrub.

## 8. Điều chưa có bằng chứng

Không có latency/throughput, index build cost, write amplification, memory budget hoặc search-quality
measurements của Cellos. Không có kết luận BFS/RedoxFS nhanh hơn CellosFS, hoặc engine nào đã port được.
Không có so sánh crash/power-loss thực nghiệm hay independent architecture approval.

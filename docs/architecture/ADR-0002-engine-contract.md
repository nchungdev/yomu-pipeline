# ADR-0002 · yomu-pipeline là engine dịch truyện độc lập, một hợp đồng, hai cách vận chuyển

- Trạng thái: Đề xuất (2026-11-10)
- Phạm vi: `yomu-pipeline` (repo này) và ứng dụng `yomu`.

## 1. Bối cảnh

Hiện tại phần gọi LLM (OCR + dịch), retry/quota và tối ưu ảnh nằm trong app yomu
(`crates/adapters/src/translation_adapters/`, `translation_queue/`). `yomu-pipeline` chỉ xoá chữ gốc và
vẽ chữ dịch (contour, inpaint, layout, render). Desktop còn có bản inpaint riêng
(`crates/ui/src/views/reader_{inpaint,flat_fill,patch}.rs`) trùng với pipeline mà mobile đang dùng.
Review ngày 2026-11-10 tìm thấy các lỗi gốc từ cách tách này:

- pipeline xoá chữ gốc nhưng không vẽ chữ dịch mà vẫn báo `ok` (thất bại im lặng);
- phân loại lỗi LLM bằng so chuỗi (`msg.contains("429")`): retry không dừng, 429 bị xếp nhầm thành quota,
  `Retry-After` sai đơn vị;
- logic đọc sidecar và xử lý ảnh bị nhân đôi giữa CLI, FFI và UI desktop.

## 2. Quyết định

1. **`yomu-pipeline` sở hữu toàn bộ việc biến danh sách ảnh thành kết quả dịch**: tiền xử lý ảnh, gọi vision LLM
   (OCR + dịch), tinh chỉnh toạ độ bằng contour, xoá chữ gốc, dàn chữ. App chỉ sở hữu UI, hàng đợi,
   consent, lưu trữ và vẽ.
2. **Một hợp đồng dữ liệu có phiên bản (`contract::v1`), hai cách vận chuyển**:
   - **Nhúng (in-process)**: không qua mạng.
   - **BE (dịch vụ HTTP)**: qua mạng. Cùng trait `TranslationEngine`, cùng kiểu dữ liệu, app không đổi code.
3. **Pipeline không trả cả trang ảnh đã xử lý.** Nó trả, cho từng vùng `Rendered`, một **patch pixel đã xoá chữ gốc**
   (cắt từ ảnh sạch ở độ phân giải gốc) kèm layout chữ; app dán patch lên **ảnh gốc của chính nó**. Nhờ vậy chất lượng
   nền luôn là ảnh gốc và chỉ tải về vài chục KB thay vì cả trang. Xoá chữ là xoá thật trên bản sao (không phải lớp phủ).
4. **Không có thất bại im lặng**: vùng không ở trạng thái `Rendered` thì `erase = None`; pipeline không bao giờ
   xoá chữ gốc rồi để trống.

## 3. Cấu trúc crate

```
yomu-pipeline-core    kiểu hợp đồng, ảnh, contour, inpaint, layout, render (thuần, không mạng)
yomu-pipeline-engine  tiền xử lý ảnh, adapter LLM, retry ngắn, điều phối   (feature `llm`)
yomu-pipeline-server  bọc engine thành dịch vụ HTTP (BE)
yomu-pipeline-client  implement TranslationEngine bằng cách gọi server
```

App nhúng `core` ở **cả hai** chế độ (để tự tô, tự inpaint cục bộ, tự vẽ chữ). Giai đoạn đầu có thể giữ một
crate duy nhất với feature flag, tách workspace khi ổn định.

## 4. Phân chia trách nhiệm

| Mảng | Pipeline | App |
|---|---|---|
| Giải mã/nén/cắt tile, hash bytes gốc, inpaint | Duy nhất | Không |
| Prompt, schema JSON, parse, adapter provider | Có | Không |
| Retry từng lần gọi HTTP, phân loại lỗi có kiểu (`ProviderError`) | Có | Không |
| Hàng đợi, ưu tiên, tạm dừng quota dài, consent | Không | Có |
| Lưu khoá (chế độ nhúng), chọn provider | Không | Có (inject theo request) |
| Cache sidecar `.yomu.json`, đồng bộ | Định nghĩa kiểu | Ghi atomic, sync |
| Vẽ chữ | Layout (+ ảnh ghép khi export) | Vẽ ở chế độ Overlay |

Quyết định retry/chờ lâu hơn khoảng 30 giây thuộc về app (có trạng thái bền); retry ngắn thuộc pipeline.

## 5. Gói dịch vụ và khoá API

| | Miễn phí | Trả phí |
|---|---|---|
| Chạy ở đâu | Nhúng | BE |
| Khoá LLM | Của user, lưu keychain/Keystore, không rời máy | Của dự án, trong secret manager của server |

Server trả phí cần: xác thực + kiểm tra quyền, giới hạn tốc độ/hạn mức theo tài khoản, đo lượng dùng,
quota toàn cục cho khoá chung, xoay khoá.

## 6. Nguyên tắc không lưu (BE)

- Xử lý hoàn toàn trong bộ nhớ; không ghi ảnh hay văn bản dịch ra đĩa; có giới hạn kích thước/thời gian.
- Nhật ký chỉ có metadata (id yêu cầu, kích thước, thời gian, mã lỗi, mã tài khoản); không có ảnh, không có chữ.
- Không cache kết quả dịch ở server. `request_id` chỉ giữ vài phút trong bộ nhớ để chống gửi trùng.
- Nhà cung cấp LLM vẫn nhận ảnh và có chính sách lưu riêng: chọn cấu hình không lưu (zero retention) nếu có,
  và nói rõ trên màn hình consent.

## 7. Hợp đồng `contract::v1` (rút gọn)

```rust
pub struct TranslateBatchRequest { schema_version, request_id, pages: Vec<PageInput>, options: TranslateOptions }
pub struct PageInput { page_id, image: ImageSource /*Path|Bytes*/, known_source_hash, prior: Option<PriorPage>,
                       context: Vec<ContextLine /*speaker, text, emotion*/> }
pub struct TranslateOptions { provider /*deprecated*/, chain: Vec<ProviderSpec>, credentials /*redacted Debug*/,
                              source_lang, target_lang, glossary /*GlossaryEntry{source,target,category,notes}*/,
                              reading_direction, render_mode, font_id, limits }
pub struct ProviderSpec { kind /*Gemini|OpenAiCompatible|CustomHttp*/, endpoint, model, languages,
                          credentials /*serde(skip), redacted Debug, ngoài cache_key*/ }
pub enum PipelineEvent { BatchStarted, PageStage, Throttled, PageDone(PageResult), PageFailed, BatchFinished }
pub struct PageResult { schema_version, page_id, source_hash, cache_key, page_px, regions, composited }
pub struct RegionResult { region_id, kind, reading_order, source_text, translated_text, confidence,
                          bounds, polygon, typography, layout: Option<TextLayoutV1>, erase: EraseSpec, status,
                          speaker, gender, age, emotion: Option<String>, intensity: Option<f32> }
pub enum RegionStatus { Rendered, Skipped{reason}, Failed{reason} }
pub enum EraseSpec { Patch{rect_norm, rect_px, format /*RawRgba|Png*/, data}, None }  // Flat/LocalInpaint đã bỏ (xem 7.1)
pub enum ProviderError { RateLimited{retry_after_ms: Option<u64>}, QuotaExhausted{reset_hint_ms: Option<u64>},
                         Auth{status: u16}, ContentBlocked{reason}, BadRequest(String), Server(u16), Network,
                         Timeout, InvalidResponse(String) }  // retry_hint() không clamp; kind_name(); dạng cũ vẫn đọc được
pub trait TranslationEngine { submit(req, sink, cancel) -> JobHandle; relayout_region(..); cache_key(..) }
```

Quy ước bắt buộc: toạ độ chuẩn hoá theo trang (0–1); `LayoutLine.origin_px` là gốc bút trên *alphabetic
baseline* tính bằng px của ảnh gốc; `cache_key = blake3(source_hash ‖ provider ‖ model ‖ prompt_version ‖
target_lang ‖ glossary_hash ‖ SCHEMA_VERSION)`. Chế độ render: `Overlay` (mặc định: patch + layout, client vẽ chữ),
`Composited` (pipeline vẽ chữ luôn, chỉ dùng để export, không đi qua mạng).

### 7.1 Quyết định về xoá chữ (đối chiếu mã nguồn 2026-11-10)

Mã hiện tại xoá **pixel thật trên một bản sao ảnh**: `erase_region` (`src/inpaint/region.rs`) dùng `StatisticalFlatFill`
(màu nền theo thống kê, không phải một màu duy nhất) rồi `smart_inpaint` khi nền phức tạp và có viền bubble. Desktop cũng
dựng một ảnh sạch nguyên trang (`views/reader_inpaint.rs`), tính một lần, cache RAM, không ghi đĩa. Vì vậy các biến thể
`Flat{color, polygon}` và `LocalInpaint{mask}` bị bỏ: chúng mất thông tin hoặc buộc mỗi client tự chứa thuật toán ảnh,
trái với mục 8. Hợp đồng chỉ còn `Patch` (hoặc `None` khi vùng không `Rendered`) và hàm `apply_erase(original, regions)`
để client có ảnh sạch nguyên trang. Quy ước toạ độ: `rect_norm` chuẩn hoá theo trang; `rect_px` theo ảnh gốc, client
scale nếu chỉ giữ bản thu nhỏ (desktop giữ tối đa 2048 px). Nhúng: patch là RGBA thô (không encode). BE: PNG.
*Đã được chủ dự án xác nhận (2026-11-10).*

### 7.2 Glossary, ngữ cảnh và cảm xúc

Hiện trạng: `SeriesGlossary` (`crates/domain/src/models/glossary.rs`) lưu theo bộ truyện, nhưng production không dùng
(`vision_prompt()` truyền rỗng); LLM đã trả `speaker/gender/age/emotion/intensity` và sidecar lưu, nhưng chỉ TTS dùng.
Hợp đồng v1 vì vậy cần: `glossary` mở rộng (thuật ngữ, nhân vật, cặp xưng hô, giọng điệu), `context` (vài câu trước kèm
người nói và cảm xúc) trong `PageInput`/`TranslateOptions`, `glossary_hash` trong `cache_key`, và kết quả trả về
`speaker/emotion/intensity` cho từng vùng để app cập nhật glossary và ngữ cảnh cho trang sau.

## 8. Xử lý ảnh (chỉ pipeline làm)

- Hash trên **bytes file gốc**; giải mã một lần, không bao giờ nén lại từ bản đã nén.
- Gửi LLM bản thu nhỏ (đề xuất cạnh dài 1600 px, JPEG q85, cần đo); trang dài (tỉ lệ ≥ 2.5) cắt tile chồng 15%.
- Chỉ dùng grayscale khi trang gần đơn sắc. Không dùng WebP (token tính theo pixel, không theo bytes).
- bbox từ LLM chỉ là gợi ý; tinh chỉnh bằng contour trên ảnh gốc, độ lệch là một phần của `confidence`.

## 9. Băng thông (ước lượng, chưa đo)

Nhúng: không truyền gì giữa app và pipeline; chỉ upload cho LLM. BE: upload ảnh thu nhỏ (khoảng 180 KB/trang),
tải về layout và patch của các vùng đã xoá (vài chục KB). Phương án "gửi ảnh nén và nhận ảnh đã ghép" bị loại vì
chất lượng kém.

## 10. Câu hỏi còn mở

1. OCR chỉ qua LLM (đề xuất v1) hay thêm detector cục bộ (đánh giá lại nếu độ chính xác bbox < ~85%).
2. SFX mặc định `Skipped{SfxPolicy}`, có tuỳ chọn bật dịch.
3. Patch là cache cục bộ tạo lại được, không đồng bộ (đề xuất).
4. Switch không nhúng engine; chỉ đọc sidecar v1 và vẽ overlay.
5. Một font duy nhất cho pipeline và app; chấp nhận lệch shaping tới 5% trước khi fallback.

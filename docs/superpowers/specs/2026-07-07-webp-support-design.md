# WebP EXIF 支持 —— 设计文档

**日期**: 2026-07-07
**范围**: 为 `nom-exif` 增加 WebP 图像的 EXIF/GPS 元数据支持（仅 EXIF，不含 XMP）。

## 1. 背景与目标

`nom-exif` 通过统一的 `MediaParser` 分发到各格式后端，按检测到的 MIME
提取 EXIF。目前支持的图像格式为 JPEG、PNG、HEIC/HEIF、AVIF、TIFF、
Phase One IIQ、Fujifilm RAF、Canon CR3。本设计新增 **WebP**。

目标：

- `MediaSource::open` / `from_memory` 能把 WebP 识别为 `MediaKind::Image`。
- `parse_exif` / `read_exif`（及其 `_async` 版本）能从 WebP 的 `EXIF`
  chunk 提取 EXIF，进而支持 `gps_info()` 等既有能力。
- 无 EXIF 的 WebP 返回 `Error::ExifNotFound`，与其他格式一致。

非目标（本期不做）：

- WebP `XMP ` chunk（作为 `ImageFormatMetadata::Webp` 暴露）—— 留待后续。
- ICCP、动画帧元数据、写入/编辑。

## 2. WebP 容器格式

WebP 基于 RIFF：

```
偏移  长度  内容
0     4     "RIFF"
4     4     文件大小 - 8 （小端 u32）
8     4     "WEBP"
12    ...   一串 chunk
```

每个 chunk：

```
0   4   FourCC（如 "VP8 " "VP8L" "VP8X" "ANIM" "ANMF" "ALPH" "ICCP" "EXIF" "XMP "）
4   4   负载大小（小端 u32，不含补齐字节）
8   ...  负载（size 字节）
    ...  若 size 为奇数，补 1 个 0 字节
```

要点：

- **字节序**：RIFF chunk 大小是**小端**（与 PNG 的大端相反）。
- **无 CRC**：PNG 每 chunk 有 4 字节 CRC，WebP 没有；奇数长度补 1 字节。
- **EXIF 负载**：裸 TIFF 流（`II`/`MM` 开头）。个别编码器会误加
  JPEG APP1 式的 `"Exif\0\0"` 前缀，需防御性剥离。
- **元数据仅见于扩展格式**：只有 VP8X 文件才有 `EXIF`/`XMP ` chunk；
  简单格式（`VP8 `/`VP8L` 紧跟 `WEBP`）没有元数据。
- **EXIF 位于图像码流之后**：遍历时必须跳过可能很大的
  `VP8 `/`VP8L`/`ANMF` 等 chunk 才能读到 `EXIF`。

## 3. 核心决策：走通用 EXIF 路径（不开 PNG 式特例）

现有 EXIF 提取有两种接线方式：

- **PNG 式特例**：`parse_exif_iter` 顶部特判 → `parse_png_exif_iter`
  以及一个 `_async` 双胞胎。PNG 需要它，是因为 PNG 还要**额外**返回
  `tEXt` chunk（自定义输出结构 `PngParseOut`）。
- **通用路径**：`parse_exif_iter` → `extract_exif_range` →
  `extract_exif_with_mime`，按 `MediaMimeImage` 匹配，返回
  `Option<&[u8]>`（EXIF 字节切片），再由 `range_to_iter` 构造
  `ExifIter`。JPEG/HEIF/TIFF/RAF/CR3(部分) 都走这里。

**决策：WebP 走通用路径。**

因为本期只要 EXIF，遍历器只需返回 EXIF 负载切片，可直接塞进
`extract_exif_with_mime` 的 match。收益：

- 复用 `extract_exif_range`、`range_to_iter` 以及**同步 + 异步两条**
  通用驱动 —— **无需编写任何异步专属代码**（对比 PNG 要写两份）。
- 与其余格式的接线方式完全一致，代码量最小。

**可行性已验证**：通用驱动 `load_and_parse_with_offset` 会把
`ParsingState` 透传给下一次 `parse` 调用，并把 `ParsingError::ClearAndSkip`
映射为 `LoopAction::Skip` → `clear_and_skip`。因此 WebP 遍历器返回的
`ClearAndSkip(n)` + `Some(WebpPastHeader)` 能被正确处理，与 PNG 跳过大
`IDAT` 的机制同源。

### `ClearAndSkip` 语义（沿用 PNG 约定）

`ClearAndSkip(n)` 含义为「从解析器当前逻辑位置再前进 n 字节」。闭包
看到的 `buf` 已偏移到该位置，所以跳过量必须**同时**覆盖遍历器在 `buf`
内已消费的字节（`cursor`）和还在 buf 之外的 chunk 字节，即
`cursor + total`，而非 `total - remaining`。

## 4. 组件拆解

### 4.1 `src/file.rs` —— MIME 检测

- 在 `MediaMimeImage` 枚举新增 `Webp` 变体。
- 新增 `check_webp(input: &[u8]) -> Result<(), ()>`：要求
  `input.len() >= 12 && &input[0..4] == b"RIFF" && &input[8..12] == b"WEBP"`。
- 接入 `TryFrom<&[u8]> for MediaMime` 的检测链（放在 `check_png` /
  `check_jpeg` 附近，各签名互不冲突，顺序无关紧要）：
  `else if check_webp(input).is_ok() { MediaMime::Image(MediaMimeImage::Webp) }`。
- 在 `file.rs` 的 `#[cfg(test)] mod tests` 内加一个用合成字节头断言
  `Image(Webp)` 的用例（无需外部样本文件）。

### 4.2 `src/webp.rs`（新文件）—— 纯函数遍历器

签名与其他格式的 `extract_exif` 对齐：

```rust
pub(crate) fn extract_exif(
    state: Option<ParsingState>,
    buf: &[u8],
) -> Result<(Option<&[u8]>, Option<ParsingState>), ParsingErrorState>
```

行为：

1. **文件头**：若 `state` 不是 `WebpPastHeader`，校验前 12 字节
   （`RIFF`…`WEBP`）；不足 12 字节 → `Need`；签名错 →
   `Failed { kind: WebpChunk }`。`cursor` 从 12 开始。若是
   `WebpPastHeader`（ClearAndSkip 之后 resume），`cursor` 从 0 开始，
   不再校验文件头。
2. **遍历 chunk**：每次先确保有 8 字节 chunk 头，否则 `Need`。读取
   FourCC 与小端 size；`total = 8 + size + (size & 1)`，用
   `checked_add` 防 32 位溢出（溢出 → `Failed`）。
   - `b"EXIF"`：若 `total > remaining` → `Need(total - remaining)`；
     否则取负载 `buf[cursor+8 .. cursor+8+size]`，若以 `b"Exif\0\0"`
     开头则剥掉前 6 字节，返回 `(Some(payload_slice), preserve_state)`。
   - 其他 FourCC：若 `total > remaining` →
     `ClearAndSkip(cursor + total)` 并回传 `Some(WebpPastHeader)`；
     否则 `cursor += total` 继续。
   - 走到 buf 末尾仍未找到 EXIF 且 chunk 已读完 → 返回
     `(None, preserve_state)`（由上层转成 `ExifNotFound`）。
3. 防御上限：对单个非 EXIF chunk 的大小不设人为上限（靠
   `ClearAndSkip` 流式跳过）；`checked_add` 保证不 panic、不回绕。

注意返回的 `Some(payload_slice)` 是 `buf` 的子切片（剥前缀也仍在 buf
内），上层 `extract_exif_range` 用 `buf.subslice_in_range` 得到
`Range`，再由 `range_to_iter` 在共享 `Bytes` 上零拷贝切片。

单元测试（照搬 `png.rs` 的合成构造器）：

- 最小 `RIFF/WEBP`（无 chunk 或仅 VP8X）→ `None`。
- 含 `EXIF` chunk → 返回正确负载切片。
- 含带 `Exif\0\0` 前缀的 `EXIF` → 前缀被剥离。
- 前面有大的 `VP8L`（不在 buf 内）→ `ClearAndSkip(cursor+total)` +
  `WebpPastHeader`。
- resume（`state = Some(WebpPastHeader)`，buf 从 chunk 边界开始）→
  不校验文件头，正常解析后续 chunk。
- 文件头被截断 → `Need`；签名错 → `Failed`。
- 奇数长度 chunk 的补齐字节被正确计入 `total`。
- `size = u32::MAX` → 不 panic（`Need`/`ClearAndSkip`/`Failed` 皆可）。

### 4.3 `src/parser.rs`

- `ParsingState` 枚举新增 `WebpPastHeader`（语义对标
  `PngPastSignature`：ClearAndSkip 之后告诉下一次调用 buf 不再以 12 字节
  文件头开头）。
- `impl Display for ParsingState` 新增对应分支。
- 小文件预填充 EOF 容忍：现有逻辑对 PNG 特判
  （`matches!(ms.mime, Image(Png))`），因为极小的 tEXt-only PNG 会在
  MIME 预填充阶段被读空、`fill_buf` 返回 `UnexpectedEof`。合成的最小
  WebP 同样很小，故把这些判断从 `Png` 扩展为 `Png | Webp`。涉及 4 处：
  - `parse_exif`（同步）
  - `parse_image_metadata`（同步）
  - `parse_exif_async`
  - `parse_image_metadata_async`

### 4.4 `src/exif.rs`

- 在 `extract_exif_with_mime` 的 match 新增：
  `MediaMimeImage::Webp => webp::extract_exif(state, buf)?`（形态与
  `heif_extract_exif` / `cr3_extract_exif` 一致，返回
  `(Option<&[u8]>, Option<ParsingState>)`）。
- 在 `extract_exif_range` 的 state→header 匹配新增
  `ParsingState::WebpPastHeader => None`（WebP EXIF 是裸 TIFF，
  `input_into_iter` 自行解析 TIFF 头，header 传 None）。
- 加入 `use crate::webp;`（或以 `crate::webp::` 全路径调用）。

### 4.5 `src/lib.rs`

- 新增 `mod webp;`（crate 私有，和 `mod png;` 一致）。

### 4.6 `src/error.rs`

- `MalformedKind` 新增 `WebpChunk` 变体（对标 `PngChunk`），并在其
  `Display`/描述表补上 `"webp chunk"`，避免 WebP 结构错误被误标。

### 4.7 文档与测试

- **README.md**：`Supported File Types` 的 Image 一行加入 WebP。
- **CHANGELOG.md**：新增条目，说明新增 WebP EXIF 支持。
- **端到端测试**：拼一个合成的 `RIFF/WEBP + VP8X + EXIF(裸 TIFF)`
  缓冲区，用 `MediaSource::from_memory` 跑 `parse_exif`（同步），断言能
  读到 EXIF；若开启 `tokio` feature，再用 `AsyncMediaSource::from_memory`
  跑 `parse_exif_async` 双胞胎，证明共享的通用路径同步/异步两侧都通。
  合成 TIFF 可复用最小 LE TIFF（`II` + 0x002A + IFD0 偏移 8 + 空 IFD0），
  参照 `png.rs` 测试里的 `minimal_tiff_le`。

## 5. 错误处理

| 情况 | 行为 |
|------|------|
| 无 `EXIF` chunk | `Error::ExifNotFound`（经 `range_to_iter`） |
| RIFF 文件头损坏/签名错 | `ParsingError::Failed { kind: WebpChunk, .. }` |
| chunk 头/负载被截断 | `ParsingError::Need(n)`，驱动补字节后重试 |
| 大的非 EXIF chunk 不在 buf 内 | `ParsingError::ClearAndSkip(cursor+total)` |
| `size` 溢出 usize（32 位） | `ParsingError::Failed { kind: WebpChunk, .. }` |

## 6. 兼容性

- 纯新增：`MediaMimeImage`、`ParsingState`、`MalformedKind` 均为
  crate 私有枚举，新增变体不影响公有 API。
- `ImageFormatMetadata` 已标 `#[non_exhaustive]`，未来加 `Webp(...)`
  变体不破坏用户的 `match`（本期不加）。
- 无新增依赖，保持纯 Rust、可交叉编译。

## 7. 验证清单

- `cargo fmt --check`、`cargo clippy`、`cargo test` 全绿。
- `webp.rs` 单测覆盖上文列出的各分支。
- `file.rs` mime 检测测试通过。
- 端到端同步 + 异步测试通过。
- README / CHANGELOG 更新到位。

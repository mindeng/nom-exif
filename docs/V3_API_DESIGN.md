# nom-exif v3 API 设计方案

> 状态：草案 v0.1
> 更新：2026-05-08
> 范围：完整重设计公开 API。允许破坏性变更（与 v2.x 不兼容）。
> 适用读者：库维护者、贡献者、需要从 v2 升级的下游用户。

---

## 1. 设计目标

v3 的目标不是堆砌新功能，而是 **收敛对外接口**，让公开符号有清晰的语义边界，去除 v2 时期累积的设计债。

具体目标：

1. **错误类型可程序化处理**——下游能用 `match` 区分"格式不识别"、"IO 错误"、"数据损坏"等不同情形。
2. **同一概念只有一种 API 风格**——`get_gps_info`、tag 索引、IFD 访问等不再因调用对象不同而有不同签名。
3. **类型系统不泄漏实现细节**——`Seekable`/`Unseekable` 这种内部 phantom 类型不再出现在用户必须书写的签名里。
4. **零 panic 路径**——除"违反类型不变量"外，公开 API 在合理输入下永不 panic。
5. **Sync/Async 共享抽象**——单一 `MediaParser` 类型，`tokio` feature 增量启用 async 方法，不强制下游二选一。
6. **入门成本低**——一行函数 `nom_exif::read_exif(path)` 满足脚本/小工具的需求；高级用户仍可通过 `MediaParser` 复用 buffer。

---

## 2. 设计原则

| 原则 | 含义 |
|------|------|
| **Closed by default** | 公开 enum 默认 closed；明确需要外部扩展时才用 `#[non_exhaustive]` |
| **No silent loss** | 任何会丢信息的转换（`IRational → URational` 等）都用 `TryFrom` |
| **Errors are values** | 不在公开 API 中 panic；不返回不会出现 `Err` 的 `Result` |
| **One concept, one shape** | 同一语义在不同类型上 API 形状一致 |
| **Hide what you don't owe** | 未来想改的内部状态一律不公开 |
| **Convenience is opt-in** | 简化函数（`read_exif` 等）是上层封装，核心仍是 `MediaParser` |

---

## 3. 核心 API 重构方案

### 3.1 顶层模块结构

```
nom_exif/
├── lib.rs                    顶层 re-exports + 便利函数
├── error.rs                  Error 与子错误类型
├── parser/
│   ├── mod.rs                MediaParser, MediaSource
│   └── source.rs             ReadKind sealed trait（如保留类型参数）
├── exif/
│   ├── mod.rs                Exif, ExifIter, ExifIterEntry
│   ├── tag.rs                ExifTag enum
│   ├── value.rs              EntryValue + accessors
│   └── gps.rs                GPSInfo, LatLng, LatRef, LonRef, AltitudeRef
├── track/
│   ├── mod.rs                TrackInfo, TrackInfoTag
└── prelude.rs                glob-import 友好导出
```

**变化点：**
- 新增 `prelude` 模块，鼓励 `use nom_exif::prelude::*` 而非 `use nom_exif::*`，避免污染下游命名空间。
- `exif` 与 `track` 拆分，避免 `lib.rs` 里 `pub use` 列表过长。

---

### 3.2 错误类型

#### v2 现状

```rust
pub enum Error {
    ParseFailed(Box<dyn std::error::Error + Send + Sync>),  // 吸尘器
    IOError(std::io::Error),
    UnrecognizedFileFormat,
}
// 所有 From 都路由到 ParseFailed，IOError 形同虚设
```

#### v3 设计

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unsupported media format")]
    UnsupportedFormat,

    #[error("no exif data found in this file")]
    ExifNotFound,

    #[error("no track info found in this file")]
    TrackNotFound,

    /// 数据被识别为目标格式，但内部结构损坏
    #[error("malformed {kind}: {message}")]
    Malformed {
        kind: MalformedKind,
        message: String,
    },

    /// 解析过程需要更多字节但流已结束
    #[error("unexpected end of input while parsing {context}")]
    UnexpectedEof { context: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MalformedKind {
    JpegSegment,
    TiffHeader,
    IfdEntry,
    IsoBmffBox,
    EbmlElement,
    Cr3Container,
    Heif,
    Raf,
}

/// 单个 Exif entry 的解析错误，用于 ExifIter 的 per-entry 错误。
///
/// 三个 variant 覆盖了实际遇到的全部 entry-level 失败模式；故意保持窄小，
/// 避免 v2 的 `String` 泛化陷阱。
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum EntryError {
    /// Entry 头部声明的字节数超出 IFD/文件实际可用范围
    #[error("entry truncated: needed {needed} bytes, only {available} available")]
    Truncated { needed: usize, available: usize },

    /// (format, count) 组合与该 tag 期望的形状不符，或 format code 未知
    #[error("invalid entry shape: format={format}, count={count}")]
    InvalidShape { format: u16, count: u32 },

    /// 字节解析成功但解码后的值对该 tag 无效
    /// （非 UTF-8 / 日期时间格式错误 / GPS ref 字符不在 N/S/E/W 内 等）。
    /// 用 `&'static str` 而非 `String`，原因列表是有限且固定的。
    #[error("invalid value: {0}")]
    InvalidValue(&'static str),
}

/// EntryError 向上传播为 Error::Malformed { kind: IfdEntry, .. }，
/// 供下游用 `?` 把 per-entry 错误冒泡到 file-level 错误。
impl From<EntryError> for Error {
    fn from(e: EntryError) -> Self {
        Error::Malformed {
            kind: MalformedKind::IfdEntry,
            message: e.to_string(),
        }
    }
}
```

#### ConvertError（peer 类型）

`ConvertError` 是字符串解析、rational 转换等"对用户输入做转换"的错误，与文件解析
正交（无 IO，无 parser session），因此与 `Error` / `EntryError` 是 **peer-level**
类型，*不* 提供 `From<ConvertError> for Error`——混用两种错误的下游函数请定义
自己的 wrapper enum（标准 `thiserror` `#[from]` 模式）。

```rust
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum ConvertError {
    #[error("unknown ExifTag name: {0}")]
    UnknownTagName(String),

    #[error("invalid ISO 6709 coordinate: {0}")]
    InvalidIso6709(String),

    #[error("rational has negative value")]
    NegativeRational,

    #[error("decimal degrees out of range or non-finite: {0}")]
    InvalidDecimalDegrees(f64),
}
```

**三类错误关系：**

```
        Error                       ConvertError
          ▲                              ▲
          │ From<EntryError>             │  (无 From into Error)
          │                              │
     EntryError                  (与文件解析正交)
   (per-entry，ExifIter 内)
```

| 错误 | 出现于 | 用户何时遇到 |
|------|--------|--------------|
| `Error` | `read_exif`、`MediaParser::parse_*`、`MediaSource::open` | 每次解析文件 |
| `EntryError` | `ExifIter` 内 per-entry yield（`ExifIterEntry::result()` 等） | 仅迭代 ExifIter 时遇到坏 entry |
| `ConvertError` | `ExifTag::from_str`、`GPSInfo::from_str`、`URational::try_from(IRational)`、`LatLng::try_from_decimal_degrees` | 仅做这些具体转换时（少见） |

**关键变化：**
- 删除 `From<&str>` / `From<String> for Error`——这些 catch-all 实现是 v2 错误失控的根源。
- `Io(#[from] io::Error)` 真正承载 IO 错误，`?` 直接传播到正确变体。
- `EntryError` 改为公开 enum，三个 variant 全部结构化（不再用 `String` 泛化），
  下游可以 `match` 出具体原因。
- `From<EntryError> for Error`：允许用户用 `?` 把 per-entry 错误传播到 file-level。
- `ConvertError`：取代 v2 中分散的转换错误（v2 通过 `crate::Error` 间接表达），
  与 `Error` 解耦。
- 删除 `Error::ParseFailed(Box<dyn Error>)` 这种 opaque 形式。

---

### 3.3 入口与 Parser 模型

#### v2 现状

```rust
pub struct MediaSource<R, S = Seekable> { ... }  // S 是 pub(crate) 类型
pub struct MediaParser { ... }
pub trait ParseOutput<R, S>: Sized { ... }       // 三参数 trait
```

问题：`Seekable`/`Unseekable` 是 `pub(crate)`，但出现在公开类型 `MediaSource<R, S>` 的签名里，下游无法书写完整类型。

#### v3 设计

**方案：消除类型参数 S，运行时决定 skip 策略。**

```rust
pub struct MediaSource<R> {
    reader: R,
    header_buf: Vec<u8>,
    mime: MediaMime,
    skip_strategy: SkipStrategy,
}

enum SkipStrategy {
    Seek,    // 通过 io::Seek::seek 跳跃
    Read,    // 通过读取并丢弃跳跃
}

impl<R: Read + Seek> MediaSource<R> {
    pub fn seekable(reader: R) -> Result<Self>;
}

impl<R: Read> MediaSource<R> {
    pub fn unseekable(reader: R) -> Result<Self>;
}

impl MediaSource<File> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self>;
    pub fn from_file(file: File) -> Result<Self>;
}

impl<R> MediaSource<R> {
    pub fn kind(&self) -> MediaKind;
    pub fn mime(&self) -> MediaMime;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// 文件含 EXIF 元数据，使用 `parse_exif` / `read_exif`。
    Image,
    /// 文件是基于时间的容器（视频、音频，或两者）。使用 `parse_track` /
    /// `read_track`。注意：纯音频容器（如 `.mka`）也归入此类。
    Track,
}

// 注：MediaKind 是封闭 enum（无 #[non_exhaustive]）。
// §8.6 论证 MediaKind 与 Metadata 永远只有两个 variant；HEIC 内嵌 MOV 等
// 场景属于 *embedded media extraction* 范畴（见 `Exif::has_embedded_media()`、
// 未来的 `MediaSource::extract_embedded()`），不通过此 enum 表达。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaMime {
    Jpeg, Heic, Heif, Tiff, Iiq, Raf, Cr3,    // image
    Mp4, Mov, ThreeGp, Webm, Matroska, Mka,   // video/audio
}
```

**变化点：**
- 删除 `MediaSource::tcp_stream`——它只是 `unseekable` 的别名，无独特行为。
- 删除 `has_exif()` / `has_track()`，统一为 `kind()`，避免"两个 bool 互斥"的隐含约定。
- 增加 `mime()` 让用户能查到具体格式（v2 内部已知但未暴露）。
- `MediaSource::open(path)` 取代 v2 的 `file_path`——名字更短更地道。
- `SkipStrategy` 是私有的（不出现在 `pub use` 中），构造时由 `seekable` /
  `unseekable` 二选一确定，**运行时不会回退**：seek 失败直接返回 `Error::Io`。
  v2 通过 `Skip` trait + `bool` 返回值允许"先尝试 seek 再 fallback 到 read"
  的隐式回退；v3 移除该回退（理由见 §7.3：静默回退会掩盖被截断的 file
  handle 等真实问题，且性能特征会突然劣化让用户难以诊断）。下游若需要双
  策略，请显式构造两个 `MediaSource`。

#### MediaParser

```rust
pub struct MediaParser { /* 内部 buffer 池，对外不可见 */ }

impl MediaParser {
    pub fn new() -> Self;

    /// 解析 Exif（适用于 image 类型的 source）
    pub fn parse_exif<R: Read>(&mut self, ms: MediaSource<R>) -> Result<ExifIter>;

    /// 解析 Track Info（适用于 video 类型的 source）
    pub fn parse_track<R: Read>(&mut self, ms: MediaSource<R>) -> Result<TrackInfo>;
}
```

**关键变化：**
- 删除 `parse<O: ParseOutput<...>>` 这种"靠类型推断决定输出"的 API。改为两个明确命名的方法。
  - 优点：调用点不需要类型注解 `let iter: ExifIter = parser.parse(ms)?`，写 `parser.parse_exif(ms)?` 即可。
  - 优点：错误信息友好（要 ExifIter 但 source 是视频会得到 `Error::ExifNotFound`，而不是泛化的 trait bound 错误）。
- 移除公开 `ParseOutput` trait——它本就是泛型分发的实现细节。

---

### 3.4 Sync / Async 整合

**取舍记录：** 用户选择"统一抽象，feature 切换"。但纯 `maybe_async` 风格会导致 sync 与 async 互斥（feature 统一问题），下游难以同时支持两种用法。

**v3 折衷：** 单一 `MediaParser` 类型，async 方法是 sync 方法的 *增量* 提供，不互斥。`MediaSource` 与 `AsyncMediaSource` 仍作为独立类型（async 读取器需要不同的 header 探测逻辑）。

```rust
// 始终可用
pub struct MediaSource<R> { ... }
impl<R: Read + Seek> MediaSource<R> { ... }

// tokio feature 启用时可用
#[cfg(feature = "tokio")]
pub struct AsyncMediaSource<R> { ... }
#[cfg(feature = "tokio")]
impl<R: tokio::io::AsyncRead + tokio::io::AsyncSeek + Unpin> AsyncMediaSource<R> { ... }

// 单一 MediaParser，方法在 R 满足不同 bound 时分别可用
pub struct MediaParser { ... }

impl MediaParser {
    pub fn parse_exif<R: Read>(&mut self, ms: MediaSource<R>) -> Result<ExifIter>;
    pub fn parse_track<R: Read>(&mut self, ms: MediaSource<R>) -> Result<TrackInfo>;

    #[cfg(feature = "tokio")]
    pub async fn parse_exif_async<R: AsyncRead + Unpin>(
        &mut self,
        ms: AsyncMediaSource<R>,
    ) -> Result<ExifIter>;

    #[cfg(feature = "tokio")]
    pub async fn parse_track_async<R: AsyncRead + Unpin>(
        &mut self,
        ms: AsyncMediaSource<R>,
    ) -> Result<TrackInfo>;
}
```

**关于 feature 命名：** v3 将 v2 的 `async` feature 改名为 `tokio`。原因：
- `async` 暗示 runtime 无关，但实际依赖只能是 tokio——名字误导。
- 参照社区惯例（reqwest 用 `default-tls`/`rustls-tls`，redis-rs 用 `tokio-comp`/`async-std-comp`），feature 名应反映具体依赖。
- 若未来支持 async-std/smol，可平行新增 `async-std` feature；届时 `tokio` 名字仍准确。

**收益：**
- 删除独立的 `AsyncMediaParser` 类型（v2 中存在的并行类型），buffer 复用逻辑只有一份。
- 内部解析逻辑通过私有 trait 抽象，sync/async 共享代码（消除 v2 中 `parser.rs` 与 `parser_async.rs` 的重复）。
- 用户工程同时需要 sync 与 async 时，启用 `tokio` feature 即可两个方法都用。

**已知代价：**
- 启用 `tokio` feature 会引入 tokio 依赖；不接受这一代价的下游用 default features 即可。

---

### 3.5 ExifIter / Exif 双 API

保留双 API，但补齐对称性。

#### Exif（eager）

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Exif { /* 内部按 IFD 索引存 */ }

impl Exif {
    /// 默认从 IFD0 取
    pub fn get(&self, tag: ExifTag) -> Option<&EntryValue>;

    /// 指定 IFD（0 = 主图，1 = thumbnail，>1 = 子 IFD）
    pub fn get_in(&self, ifd: IfdIndex, tag: ExifTag) -> Option<&EntryValue>;

    /// 兜底：raw u16 tag code（用于未识别 tag）
    pub fn get_by_code(&self, ifd: IfdIndex, code: u16) -> Option<&EntryValue>;

    pub fn gps_info(&self) -> Option<&GPSInfo>;

    /// 遍历所有 entry。需要按 IFD 过滤时用 `iter().filter(|e| e.ifd == ..)`，
    /// 不再单独提供 `iter_ifd` / `ifds` 便利方法（一行 filter/collect 即可，
    /// 不值得占用公开 API 表面）。
    pub fn iter(&self) -> impl Iterator<Item = ExifEntry<'_>>;

    /// 由 `From<ExifIter> for Exif` 转换时丢弃的 per-entry 错误。
    /// 用户可以通过此方法定位坏 entry 而不必走 lazy 路径。
    pub fn errors(&self) -> &[(IfdIndex, TagOrCode, EntryError)];

    /// 此文件是否嵌入了未被本次解析处理的额外媒体流
    /// （例如 HEIC Live Photo 的 MOV、RAF 的 JPEG preview）。
    /// 见 §8.6 关于 embedded media 的说明。
    pub fn has_embedded_media(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IfdIndex(usize);

impl IfdIndex {
    pub const MAIN: Self = IfdIndex(0);
    pub const THUMBNAIL: Self = IfdIndex(1);

    pub const fn new(index: usize) -> Self;
    pub const fn get(self) -> usize;
}

/// Exif::iter() 的 item，零拷贝引用 entry
#[derive(Clone, Copy, Debug)]
pub struct ExifEntry<'a> {
    pub ifd: IfdIndex,
    pub tag: TagOrCode,
    pub value: &'a EntryValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagOrCode {
    Tag(ExifTag),
    Unknown(u16),
}
```

**关键变化：**
- `get_gps_info` 改名为 `gps_info`，返回 `Option<&GPSInfo>`（删除 v2 中永远 `Ok` 的 `Result` 包装）。
- 增加 `iter()`——v2 中转成 `Exif` 后就无法遍历，这是空白。按 IFD 过滤或收集 IFD 列表都用 `iter()` 一行 filter/collect 实现，不再单独暴露 `iter_ifd` / `ifds`。
- `IfdIndex` newtype 替代魔术数字 `usize`，并提供 `MAIN`/`THUMBNAIL` 常量。
- `TagOrCode` 替代 v2 内部的 `ExifTagCode`（v2 是 `pub(crate)` 但其值通过 `tag()` / `tag_code()` 间接泄漏到公开 API）。

#### ExifIter（lazy）

```rust
pub struct ExifIter { ... }

impl ExifIter {
    pub fn rewind(&mut self);
    pub fn clone_rewound(&self) -> Self;
    pub fn parse_gps(&self) -> Result<Option<GPSInfo>>;

    /// 与 `Exif::has_embedded_media()` 同义；header 解析后即可返回，
    /// 不需要驱动迭代器。
    pub fn has_embedded_media(&self) -> bool;
}

impl Iterator for ExifIter {
    type Item = ExifIterEntry;
}

/// `From<ExifIter> for Exif` 会驱动迭代器、按 IFD 收集成功的 entry，并把
/// per-entry 错误存入 `Exif::errors()`（不丢弃）。
impl From<ExifIter> for Exif {
    fn from(iter: ExifIter) -> Self;
}

pub struct ExifIterEntry {
    /* 不再有 take_value/take_result panic 路径 */
}

impl ExifIterEntry {
    pub fn ifd(&self) -> IfdIndex;
    pub fn tag(&self) -> TagOrCode;       // 替代 tag()/tag_code() 双方法
    pub fn value(&self) -> Option<&EntryValue>;
    pub fn error(&self) -> Option<&EntryError>;
    pub fn result(&self) -> Result<&EntryValue, &EntryError>;
    pub fn into_result(self) -> Result<EntryValue, EntryError>;  // 替代 take_result
}
```

**关键变化：**
- `ParsedExifEntry` 改名为 `ExifIterEntry`：与容器类型 `ExifIter` 配对，
  避免与 eager 路径的 `ExifEntry` 名字混淆（"Parsed" 前缀其实没区分作用，
  两者都是已 parse 的 entry，差别在于"有效视图" vs "可能出错的尝试"）。
- 删除 `take_value` / `take_result`——这两个方法在 v2 里第二次调用会 panic，是隐藏陷阱。`into_result` 消费 self 即可避免该问题。
- `tag()` 返回 `TagOrCode` 替代 v2 里的 `Option<ExifTag>` + `tag_code()` 双方法，更直接。
- `clone_and_rewind` 改为更地道的 `clone_rewound`（动词→形容词，符合 Rust 惯例）。
- `parse_gps_info` 简化为 `parse_gps`。
- `From<ExifIter> for Exif` 不丢弃 per-entry 错误：成功的 entry 进入主存储，
  失败的进入 `Exif::errors()`，eager/lazy 两条路径访问同一份信息。
- `ExifEntry` 与 `ExifIterEntry` 都使用 plain pub fields（不加
  `#[non_exhaustive]`）：字段集稳定，未来若真要加字段就升 v4。

---

### 3.6 EntryValue

#### 补齐访问器

```rust
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EntryValue {
    // 标量
    Text(String),
    U8(u8), U16(u16), U32(u32), U64(u64),
    I8(i8), I16(i16), I32(i32), I64(i64),
    F32(f32), F64(f64),
    URational(URational),
    IRational(IRational),

    // 时间
    DateTime(DateTime<FixedOffset>),
    NaiveDateTime(NaiveDateTime),

    // 二进制
    Undefined(Vec<u8>),

    // 数组
    U8Array(Vec<u8>),
    U16Array(Vec<u16>),
    U32Array(Vec<u32>),
    URationalArray(Vec<URational>),
    IRationalArray(Vec<IRational>),
}

impl EntryValue {
    // —— 标量访问器（补齐到 14 个）——
    pub fn as_str(&self) -> Option<&str>;
    pub fn as_u8(&self) -> Option<u8>;
    pub fn as_u16(&self) -> Option<u16>;
    pub fn as_u32(&self) -> Option<u32>;
    pub fn as_u64(&self) -> Option<u64>;
    pub fn as_i8(&self) -> Option<i8>;
    pub fn as_i16(&self) -> Option<i16>;
    pub fn as_i32(&self) -> Option<i32>;
    pub fn as_i64(&self) -> Option<i64>;       // 新增
    pub fn as_f32(&self) -> Option<f32>;       // 新增
    pub fn as_f64(&self) -> Option<f64>;       // 新增
    pub fn as_urational(&self) -> Option<URational>;
    pub fn as_irational(&self) -> Option<IRational>;

    // —— 数组访问器 ——
    pub fn as_u8_slice(&self) -> Option<&[u8]>;
    pub fn as_u16_slice(&self) -> Option<&[u16]>;          // 新增
    pub fn as_u32_slice(&self) -> Option<&[u32]>;          // 新增
    pub fn as_urational_slice(&self) -> Option<&[URational]>;
    pub fn as_irational_slice(&self) -> Option<&[IRational]>;
    pub fn as_undefined(&self) -> Option<&[u8]>;            // 新增

    // —— 时间访问器（核心改进）——
    /// EXIF 中的日期时间可能带时区（`DateTime<FixedOffset>`）也可能不带
    /// （`NaiveDateTime`）。返回 `DateTimeValue` 包装，让类型如实反映这一点。
    pub fn as_datetime(&self) -> Option<DateTimeValue>;

    // —— 弱类型转换（自动跨数值类型转换）——
    /// 任何整数类型 → i64（widening）
    pub fn try_as_integer(&self) -> Option<i64>;

    /// 任何 rational/float/integer → f64
    pub fn try_as_float(&self) -> Option<f64>;
}

/// EXIF datetime 的两种形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateTimeValue {
    /// 原始值携带时区（如 `OffsetTimeOriginal` 拼装而来）
    Aware(DateTime<FixedOffset>),
    /// 原始值不带时区（如裸的 `DateTime` tag）
    Naive(NaiveDateTime),
}

impl DateTimeValue {
    /// 仅当原值带时区时返回；否则 None。
    pub fn aware(&self) -> Option<DateTime<FixedOffset>>;

    /// 总是返回 NaiveDateTime；带时区的会被剥离时区。
    pub fn into_naive(self) -> NaiveDateTime;

    /// 不带时区时套用 fallback 偏移；带时区时保留原偏移。
    pub fn or_offset(self, fallback: FixedOffset) -> DateTime<FixedOffset>;
}
```

**关键变化：**
- 补齐之前缺失的 `as_i64` / `as_f32` / `as_f64` / `as_u16_slice` / `as_u32_slice` / `as_undefined`。
- **单一 `as_datetime() -> Option<DateTimeValue>`**：v2 的 `as_time_components` 返回 `(NaiveDateTime, Option<FixedOffset>)` 强迫用户做拼装。原 v3 草案拆成 `as_datetime` + `as_naive_datetime` 两个方法又会让"任何 datetime"的取值要 fallback；最终采用 `DateTimeValue` enum，类型如实反映"EXIF datetime 可能带或不带时区"，三类调用方各取所需：`val.as_datetime()?.aware()`、`?.into_naive()`、`?.or_offset(fallback)`。
- 增加 `try_as_integer` / `try_as_float` 跨类型方便访问器，避免用户需要分别匹配 `as_u8`/`as_u16`/...。
- 数组访问器统一以 `_slice` 结尾，`as_u8array` 这种风格丢弃。
- 删除有风险的 `From<DateTime<Utc>>`（含运行时 `assert_eq!`）和冗余的 `From<&String>`。

---

### 3.7 ExifTag 与未识别 tag

保持 closed enum + raw u16 兜底。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExifTag {
    Make = 0x010f,
    Model = 0x0110,
    // ... 70+ 已识别 tag
}

impl ExifTag {
    pub const fn code(self) -> u16;
    pub const fn name(self) -> &'static str;
}

impl ExifTag {
    /// 从 raw code 反查。未知返回 None。
    pub fn from_code(code: u16) -> Option<Self>;
}

impl FromStr for ExifTag {
    type Err = ConvertError;
    fn from_str(s: &str) -> Result<Self, Self::Err>;  // ConvertError::UnknownTagName
}

impl fmt::Display for ExifTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}
```

**关键变化：**
- 删除 `TryFrom<u16> for ExifTag`（v2 里返回 `crate::Error`——错误类型过重）。换成 `from_code(u16) -> Option<Self>`，语义更准确。
- 增加 `FromStr for ExifTag`，与 `TrackInfoTag` 对称；`Err = ConvertError`（统一的转换错误，见 §3.2）。
- 删除 v2 内部的 `From<ExifTag> for &str`（功能与 `name()` / `Display` 重叠）。
- 用户访问未识别 tag 通过 `Exif::get_by_code(ifd, raw_code)` 或 `ExifIterEntry::tag() == TagOrCode::Unknown(code)`。

---

### 3.8 Rational / URational / IRational

#### v2 现状

```rust
pub struct Rational<T>(pub T, pub T);  // 元组字段公开
pub type URational = Rational<u32>;
pub type IRational = Rational<i32>;

impl From<IRational> for URational {  // ⚠️ 用 as u32，负数 silent truncate
    fn from(value: IRational) -> Self {
        Self(value.0 as u32, value.1 as u32)
    }
}
```

#### v3 设计

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rational<T> {
    numerator: T,
    denominator: T,
}

pub type URational = Rational<u32>;
pub type IRational = Rational<i32>;

impl<T: Copy> Rational<T> {
    pub const fn new(numerator: T, denominator: T) -> Self;
    pub const fn numerator(&self) -> T;
    pub const fn denominator(&self) -> T;
    pub const fn into_parts(self) -> (T, T);
}

impl<T: Copy + Into<f64>> Rational<T> {
    /// 仅当 denominator != 0 时返回浮点值
    pub fn to_f64(&self) -> Option<f64>;
}

impl<T: Copy> From<(T, T)> for Rational<T> { /* ... */ }
impl<T: Copy> From<Rational<T>> for (T, T) { /* ... */ }

// 替代 v2 中危险的 From<IRational> for URational
impl TryFrom<IRational> for URational {
    type Error = ConvertError;
    fn try_from(value: IRational) -> Result<Self, Self::Error>;  // ConvertError::NegativeRational
}
```

**关键变化：**
- 隐藏字段，提供 `numerator()` / `denominator()`——v2 中 `r.0` / `r.1` 可读性很差。
- `to_f64()` 返回 `Option<f64>`，分母为 0 时返回 `None`（v2 中 `as_float()` 会返回 inf/nan）。
- `From<IRational> for URational` 改为 `TryFrom`，避免 silent truncation；错误类型用统一的 `ConvertError`（见 §3.2）。

---

### 3.9 GPSInfo / LatLng

保留 EXIF 原始结构，但用 enum 替代 char/u8 魔术值，并加便利访问器。

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GPSInfo {
    pub latitude_ref: LatRef,
    pub latitude: LatLng,
    pub longitude_ref: LonRef,
    pub longitude: LatLng,
    pub altitude: Altitude,
    pub speed: Option<Speed>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatRef { North, South }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LonRef { East, West }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Altitude {
    Unknown,
    AboveSeaLevel(URational),
    BelowSeaLevel(URational),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedUnit { KmPerHour, MilesPerHour, Knots }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Speed {
    pub unit: SpeedUnit,
    pub value: URational,
}

/// 度 / 分 / 秒
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LatLng {
    pub degrees: URational,
    pub minutes: URational,
    pub seconds: URational,
}

impl LatLng {
    pub const fn new(degrees: URational, minutes: URational, seconds: URational) -> Self;

    /// 转为十进制度数
    pub fn to_decimal_degrees(&self) -> Option<f64>;

    /// 从十进制度数构造。NaN/±inf/超出 ±180° 等返回
    /// `ConvertError::InvalidDecimalDegrees`，避免 v2 `From<f64>` 静默接受
    /// 非法值的问题（§2 "no silent loss" 原则）。
    pub fn try_from_decimal_degrees(degrees: f64) -> Result<Self, ConvertError>;
}

impl GPSInfo {
    pub fn latitude_decimal(&self) -> Option<f64>;
    pub fn longitude_decimal(&self) -> Option<f64>;
    pub fn altitude_meters(&self) -> Option<f64>;

    /// ISO 6709 字符串格式
    pub fn to_iso6709(&self) -> String;
}

impl FromStr for GPSInfo {
    type Err = ConvertError;
    fn from_str(s: &str) -> Result<Self, Self::Err>;  // ConvertError::InvalidIso6709
}
```

**关键变化：**
- `latitude_ref: char` → `LatRef` enum，不再可能传入 `'X'` 这种非法值。
- `altitude_ref: u8 + altitude: URational` → `Altitude` enum，把 ref 与 value 绑成不可分离的整体。
- `speed_ref + speed` 同理合并为 `Option<Speed>`。
- `LatLng` 改为命名字段（`degrees`/`minutes`/`seconds`），不再是 `LatLng(URational, URational, URational)` 元组——可读性显著提升。
- 删除大量 `FromIterator`/`From<f64>` 实现（v2 有 `unwrap()` panic 风险）。仅保留 `try_from_decimal_degrees`（fallible）和 `new`。
- `format_iso6709` → `to_iso6709`（与 Rust 标准的 `to_*` 命名一致）。
- `FromStr for GPSInfo` 错误类型改为统一的 `ConvertError`（见 §3.2）；
  v2 的 `InvalidISO6709Coord` 与 v3 草案的 `Iso6709ParseError` 都不再单独存在。

---

### 3.10 TrackInfo / TrackInfoTag

主要做小修补，不大改。

```rust
#[derive(Debug, Clone, Default)]
pub struct TrackInfo { /* ... */ }

impl TrackInfo {
    pub fn get(&self, tag: TrackInfoTag) -> Option<&EntryValue>;
    pub fn gps_info(&self) -> Option<&GPSInfo>;     // 改名（去掉 get_）
    pub fn iter(&self) -> impl Iterator<Item = (TrackInfoTag, &EntryValue)>;

    /// 此容器是否嵌入了未被本次解析处理的额外媒体流。与 `Exif::has_embedded_media()`
    /// 对称（如 mka 含视频流而本次只解析了 audio track）。见 §8.6。
    pub fn has_embedded_media(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum TrackInfoTag {
    Make, Model, Software, CreateDate,
    DurationMs, ImageWidth, ImageHeight,
    GpsIso6709, Author,
}

impl TrackInfoTag {
    pub const fn name(self) -> &'static str;
}

impl FromStr for TrackInfoTag { /* ... */ }
impl fmt::Display for TrackInfoTag { /* ... */ }
```

**关键变化：**
- `get_gps_info` → `gps_info`（与 Exif 对齐，去掉冗余 `get_` 前缀）。
- 删除 `From<TrackInfoTag> for &str`，统一通过 `name()` 或 `Display`。
- 删除 `From<BTreeMap<...>> for TrackInfo` 公开转换——这是构造细节，不应在公开 API。
- 删除 `IntoIterator` 实现（用 `iter()` 替代，避免 owned iteration 的所有权陷阱）。

---

### 3.11 顶层便利函数

新增一组零样板的便利函数，覆盖最常见的"读取一个文件"场景。

```rust
// nom_exif/lib.rs

/// 读取文件的 Exif 数据，按需要 eager 解析为 Exif。
///
/// 内部对 `File` 套 `BufReader` 以避免每次 read 都触发 syscall——单文件
/// 调用点的 hot path（`for path in paths { read_exif(path)? }`）由此免疫
/// 朴素未缓冲的性能陷阱。批量场景仍建议直接用 `MediaParser` 复用 buffer。
pub fn read_exif(path: impl AsRef<Path>) -> Result<Exif>;

/// 读取文件的 Exif 数据为 lazy 迭代器。同样内部包 BufReader。
pub fn read_exif_iter(path: impl AsRef<Path>) -> Result<ExifIter>;

/// 读取视频/音频文件的 track 元数据。同样内部包 BufReader。
pub fn read_track(path: impl AsRef<Path>) -> Result<TrackInfo>;

/// 自动判别 image / video 并返回统一结果。
pub fn read_metadata(path: impl AsRef<Path>) -> Result<Metadata>;

#[derive(Debug, Clone)]
pub enum Metadata {
    Exif(Exif),
    Track(TrackInfo),
}

// 注：Metadata 同样不提供 Both variant（无 #[non_exhaustive]，封闭 enum），
// 理由与 MediaKind 一致——当前 parser 的 MIME 探测层就是二选一，加 Both
// 会成为永远不返回的死代码 API。
//
// 用户判断"是否还有未解析的元数据"通过 `Exif::has_embedded_media()` 或
// `TrackInfo::has_embedded_media()`（如 HEIC Live Photo）。真有"既需要 Exif
// 又需要 track"的场景，调用方应分别调用 read_exif 与 read_track 并各自处理
// ExifNotFound/TrackNotFound 错误。

// async 版本（feature = "tokio"）
#[cfg(feature = "tokio")]
pub async fn read_exif_async(path: impl AsRef<Path>) -> Result<Exif>;
#[cfg(feature = "tokio")]
pub async fn read_track_async(path: impl AsRef<Path>) -> Result<TrackInfo>;
#[cfg(feature = "tokio")]
pub async fn read_metadata_async(path: impl AsRef<Path>) -> Result<Metadata>;
```

**用法对比：**

```rust
// v2: 4 行 + 类型注解
let mut parser = MediaParser::new();
let ms = MediaSource::file_path("photo.jpg")?;
let iter: ExifIter = parser.parse(ms)?;
let exif: Exif = iter.into();

// v3: 1 行
let exif = nom_exif::read_exif("photo.jpg")?;
```

---

## 4. 完整公开符号清单

### 4.1 模块 `nom_exif`（顶层 re-exports）

| 符号 | 类型 | 来源 |
|------|------|------|
| `MediaParser` | struct | parser |
| `MediaSource<R>` | struct | parser |
| `AsyncMediaSource<R>` | struct (feature) | parser |
| `MediaKind` | enum | parser |
| `MediaMime` | enum | parser |
| `Exif` | struct | exif |
| `ExifIter` | struct | exif |
| `ExifEntry<'a>` | struct | exif |
| `ExifIterEntry` | struct | exif |
| `ExifTag` | enum | exif |
| `IfdIndex` | struct | exif |
| `TagOrCode` | enum | exif |
| `EntryValue` | enum | exif |
| `DateTimeValue` | enum | exif |
| `EntryError` | enum | exif |
| `URational` / `IRational` / `Rational<T>` | type/struct | exif |
| `GPSInfo` | struct | exif::gps |
| `LatLng` | struct | exif::gps |
| `LatRef` / `LonRef` | enum | exif::gps |
| `Altitude` | enum | exif::gps |
| `Speed` / `SpeedUnit` | struct/enum | exif::gps |
| `TrackInfo` | struct | track |
| `TrackInfoTag` | enum | track |
| `Metadata` | enum | lib |
| `Error` | enum | error |
| `MalformedKind` | enum | error |
| `ConvertError` | enum | error |
| `Result<T>` | type alias | error |
| `read_exif` / `read_exif_iter` / `read_track` / `read_metadata` | fn | lib |
| `read_*_async` | fn (feature) | lib |

**总计：约 30 个公开符号**（v2 约 16 个，但内部有不少 `pub(crate)` 类型间接通过 `pub` API 泄漏）。`UnknownTagName` / `Iso6709ParseError` / `NegativeRational` 三个一字段错误结构合并为统一的 `ConvertError` enum。

### 4.2 模块 `nom_exif::prelude`

```rust
pub use crate::{
    EntryValue, Error, Exif, ExifIter, ExifTag, GPSInfo,
    IfdIndex, MalformedKind, MediaKind, MediaParser, MediaSource,
    Metadata, Result, TrackInfo, TrackInfoTag,
};
pub use crate::{read_exif, read_metadata, read_track};
```

prelude 包含 `Error` 与 `MalformedKind`：用户匹配错误时不必再显式导入。冷门类型（`Rational`、`LatLng`、`ConvertError`、`DateTimeValue` 等）保持 `nom_exif::Type` 显式导入。

---

## 5. v2 → v3 迁移指南

### 5.1 入口与解析

| v2 | v3 |
|----|-----|
| `MediaSource::file_path(p)` | `MediaSource::open(p)` 或 `read_exif(p)` |
| `MediaSource::tcp_stream(s)` | `MediaSource::unseekable(s)` |
| `ms.has_exif()` | `ms.kind() == MediaKind::Image` |
| `ms.has_track()` | `ms.kind() == MediaKind::Track`（注意 `Video` 改名为 `Track`，纯音频容器如 `.mka` 也归入此类） |
| `parser.parse::<_, _, ExifIter>(ms)` | `parser.parse_exif(ms)` |
| `parser.parse::<_, _, TrackInfo>(ms)` | `parser.parse_track(ms)` |
| `MediaSource<R, S>` 类型参数 | `MediaSource<R>`（S 已删除） |
| 隐式 seek-fallback-to-read（v2 `Skip` trait `bool` 返回值） | 已移除：seek 失败直接 `Error::Io`（§3.3、§7.3） |

### 5.2 错误处理

| v2 | v3 |
|----|-----|
| `Error::ParseFailed(box)` | 改用结构化变体 `Malformed { kind, message }` 或 `UnexpectedEof`、`UnsupportedFormat` |
| `Error::IOError(e)` | `Error::Io(e)`（名字精简） |
| `From<&str> for Error` | 已删除——内部错误请用具体变体 |
| `EntryError`（包私有 enum，含 `String` 字段） | `EntryError`（公开 enum，3 个结构化 variant：`Truncated` / `InvalidShape` / `InvalidValue(&'static str)`） |
| 无 entry-level → file-level 错误传播 | `From<EntryError> for Error`（映射为 `Malformed { kind: IfdEntry, .. }`） |
| 字符串/rational 转换错误散落于 `crate::Error` 或独立类型 | 统一为 `ConvertError`（peer-level，与 `Error` 不互转）|

### 5.3 EntryValue

| v2 | v3 |
|----|-----|
| `value.as_time_components() -> Option<(NaiveDateTime, Option<FixedOffset>)>` | `value.as_datetime() -> Option<DateTimeValue>`（`DateTimeValue::Aware`/`Naive`，配 `aware()`/`into_naive()`/`or_offset(fallback)` 三个用法） |
| `value.as_u8array()` | `value.as_u8_slice()` |
| `value.to_u8array()` | （删除——使用 `as_u8_slice().map(<[u8]>::to_vec)`） |
| 缺少 `as_i64`/`as_f32`/`as_f64`/`as_u16_slice`/... | 已补齐 |

### 5.4 ExifTag

| v2 | v3 |
|----|-----|
| `ExifTag::try_from(0x010f)` | `ExifTag::from_code(0x010f)` |
| `<&str as From<ExifTag>>::from(t)` | `t.name()` 或 `t.to_string()` |
| 没有 `&str → ExifTag` | `ExifTag::from_str("Make")` |

### 5.5 Exif / ExifIter

| v2 | v3 |
|----|-----|
| `exif.get_gps_info()? -> Option<GPSInfo>` | `exif.gps_info() -> Option<&GPSInfo>` |
| `exif.get_by_ifd_tag_code(0, 0x0110)` | `exif.get_by_code(IfdIndex::MAIN, 0x0110)` |
| `exif.get_by_ifd_tag_code(ifd, ExifTag::Make.code())` | `exif.get_in(IfdIndex::new(ifd), ExifTag::Make)`（`IfdIndex` 内部字段已私有，需用 `new`/常量构造） |
| 不能遍历 `Exif` | `exif.iter()`（按 IFD 过滤：`exif.iter().filter(\|e\| e.ifd == IfdIndex::MAIN)`） |
| 不能从 `Exif` 取出 per-entry 错误 | `exif.errors() -> &[(IfdIndex, TagOrCode, EntryError)]` |
| `ParsedExifEntry`（lazy iter 的 yield 类型） | 改名 `ExifIterEntry`（与 `ExifIter` 配对） |
| `entry.tag()` + `entry.tag_code()` | `entry.tag() -> TagOrCode` |
| `entry.take_value()` | `entry.into_result().ok()` 或先 clone |
| `entry.take_result()`（panic 风险） | `entry.into_result()`（消费 self） |
| `iter.clone_and_rewind()` | `iter.clone_rewound()` 或 `let mut x = iter.clone(); x.rewind();` |
| `iter.parse_gps_info()` | `iter.parse_gps()` |
| 无 | 新增 `Exif::has_embedded_media()` / `ExifIter::has_embedded_media()`（判断是否嵌入了未解析的额外媒体流，如 HEIC Live Photo） |

### 5.6 GPSInfo

```rust
// v2
let g = exif.get_gps_info()?.unwrap();
if g.latitude_ref == 'N' { ... }
let alt_above = g.altitude_ref == 0;

// v3
let g = exif.gps_info().unwrap();
if matches!(g.latitude_ref, LatRef::North) { ... }
let alt_above = matches!(g.altitude, Altitude::AboveSeaLevel(_));
```

### 5.7 Rational

```rust
// v2
let r = URational(1, 2);
let f = r.0 as f64 / r.1 as f64;

// v3
let r = URational::new(1, 2);
let f = r.to_f64().unwrap();   // 处理 denominator == 0 的情况

// IRational → URational（v2 silent truncate 负数；v3 显式失败）
let u: URational = ir.try_into()?;  // ConvertError::NegativeRational
```

```rust
// LatLng 由十进制度数构造
// v2
let p = LatLng::from(43.5_f64);  // 内部 unwrap 可能 panic

// v3
let p = LatLng::try_from_decimal_degrees(43.5)?;  // ConvertError::InvalidDecimalDegrees
```

### 5.8 Async

```rust
// v2
let mut parser = AsyncMediaParser::new();
let ms = AsyncMediaSource::file_path("a.jpg").await?;
let iter: ExifIter = parser.parse(ms).await?;

// v3
let mut parser = MediaParser::new();
let ms = AsyncMediaSource::open("a.jpg").await?;
let iter = parser.parse_exif_async(ms).await?;

// 或直接
let exif = nom_exif::read_exif_async("a.jpg").await?;
```

### 5.9 Cargo features

| v2 | v3 |
|----|-----|
| `nom-exif = { version = "2", features = ["async"] }` | `nom-exif = { version = "3", features = ["tokio"] }` |
| `nom-exif = { version = "2", features = ["json_dump"] }` | `nom-exif = { version = "3", features = ["serde"] }` |

Feature 改名理由见 §8.7（`async` → `tokio`）和 §8.8（`json_dump` → `serde`）。语义和功能不变，仅名字调整。

---

## 6. 内部架构影响（非 API 但需配套）

虽然本文档聚焦公开接口，以下内部变化是 v3 的必要配套：

1. **去重 sync/async 解析逻辑**：v2 中 `parser.rs` 与 `parser_async.rs` 大量重复。v3 通过私有 `BufLoader` trait（含 sync/async 两套实现，但解析主逻辑共享）合并。
2. **`PartialVec` / `Buffers` 全部 `pub(crate)`**：v2 中通过 `ExifIter::add_tiff_block` 等方法间接出现在 API 边界。
3. **`ParsingError` / `ParsingErrorState`**：完全私有，不再在公开 trait 边界出现。
4. **`Mime`**：改名 `MediaMime`，公开。
5. **`TiffHeader`**：保持 `pub(crate)`。
6. **MSRV**：建议从 1.80 升到 1.83+（用 `expect` lint、`Option::is_some_and` 等）。

---

## 7. 开放问题 / 已决议项

以下问题在 2026-05-08 的设计 review 中均已决议。后续如发现实现障碍或新证据，可重新讨论。

1. ~~**`Exif::iter()` 的 item 类型？**~~ **已决议**：保持 `ExifEntry<'a>` struct（零拷贝引用），不退化为元组。理由：未来扩展（增加 IFD 内偏移量、原始字节等字段）不需要破坏 API；用户用 `entry.tag` / `entry.value` 比 `entry.0` / `entry.1` 可读性更好。
2. ~~**`Metadata::Both` variant？**~~ **已决议**：不加。`MediaKind` 与 `Metadata` 都保持二选一（封闭 enum，无 `#[non_exhaustive]`）。HEIC 内嵌 MOV 等场景属于 *embedded media extraction* 范畴：当前通过 `Exif::has_embedded_media()` / `ExifIter::has_embedded_media()` / `TrackInfo::has_embedded_media()` 让用户感知"还有未解析的数据"，未来通过独立 API（如 `MediaSource::extract_embedded()`）暴露具体流。详见 §8.6。
3. ~~**`MediaSource` 的 skip fallback？**~~ **已决议**：`seek` 失败时返回 `Error::Io(...)`，不静默回退到 `Read`。理由：静默回退会掩盖真实问题（例如调用方传入了被截断的 file handle），且性能特征会突然劣化让用户难以诊断。
4. ~~**`async` feature 命名 / 多 runtime 支持？**~~ **已决议**：feature 改名为 `tokio`（不再用误导性的 `async`）。v3 仅支持 tokio，未来如需 async-std/smol 平行新增对应 feature。详见 §8.7。
5. ~~**`json_dump` feature？**~~ **已决议**：feature 改名为 `serde`（与生态惯例对齐）。仍直接派生 `Serialize` / `Deserialize`，不抽象为 `to_json` 方法——保持下游灵活性最大（任何 serde-compatible 格式都能用，不锁死 JSON）。详见 §8.8。
6. ~~**MakerNote per-vendor 解析？**~~ **已决议**：不在 v3 范围。v3 仍仅返回 `EntryValue::Undefined(Vec<u8>)`，下游可自行解析或使用专门的 makernote crate。理由：每家厂商的 makernote 格式不公开且变化频繁，做不好会成长期维护负担；v3 的核心目标是收敛 API，而非扩功能。可作为 v3.x 增量特性（如新增 `makernote` feature）。
7. ~~**写入支持？**~~ **已决议**：v3 仍是只读。写入需要重新设计整个数据流（解析 → 修改 → 重写），与 v3 的"读取 API 收敛"目标正交。作为 v4 议题。

---

## 8. 设计决策记录

本节记录设计过程中关键的"为什么这样而不那样"，便于未来维护者理解。

### 8.1 为什么不彻底统一 sync/async（maybe-async 风格）

最初考虑过 `maybe-async` 风格——单一 API，feature 切换决定 sync 或 async。最终放弃，因为：

- Cargo feature 是 *additive* 的：不同 crate 引入 nom-exif 时，只要有一个启用 async，所有 crate 都会得到 async 版本。这会让选择 sync 的 crate 编译失败或行为变化。
- 折衷：保留两个 source 类型（`MediaSource` 与 `AsyncMediaSource`），但统一 `MediaParser` 的实现，让两种方法共存。

### 8.2 为什么删除 `MediaSource::tcp_stream`

它只是 `unseekable` 的别名，没有 TcpStream 特有逻辑。保留只会让 API 表面增大并诱导用户以为有特殊处理。

### 8.3 为什么 `Exif::get` 默认查 IFD0 而非"任意 IFD"

EXIF 规范中 90% 的实用 tag 都在 IFD0（主图），thumbnail (IFD1) 极少需要。让 `get(tag)` 默认查 IFD0 是符合直觉的"最短路径"。需要跨 IFD 时显式调用 `get_in(ifd, tag)`。

### 8.4 为什么 `IfdIndex` 是 newtype 而不是 enum

EXIF 子 IFD 数量是开放的（某些相机有 SubIFD2/3...），用 enum 限制变体数会与 CR3 等多块格式冲突。newtype 配 `MAIN`/`THUMBNAIL` 常量兼顾类型安全和扩展性。

### 8.5 为什么 GPS 保留度分秒结构

调用方常需要原始数据（写回 EXIF、与其他工具交互）。`to_decimal_degrees()` 提供便利访问，但底层结构不丢失精度。同时这避免了 v2 的 `From<f64>` 精度损失风险。

### 8.6 为什么 `MediaKind` / `Metadata` 不加 `Both` variant

理论上 HEIC 等格式可同时携带 Exif 与内嵌视频 track（典型例：Apple Live Photos），但 v3 仍然把 `MediaKind` 与 `Metadata` 设为二选一。理由：

- **当前 MIME 探测层就是二选一**：`MediaKind::Image | Track`（基于 `MediaMime` 分类），HEIC 会被归为 `Image`，parser 不会去解析其内嵌的 MOV 流。即使 enum 加 `Both`，目前没有任何代码路径会构造它——这是死代码。
- **加 `Both` 让所有调用方付出代价**：`match` 需要多一条分支，但 99% 的文件只有一种元数据。便利性反而下降。
- **正确的解法是 *embedded media extraction***：未来通过独立 API（如 `MediaSource::extract_embedded() -> impl Iterator<Item = MediaSource>`）暴露内嵌流，由用户对每个流单独 `parse_exif` / `parse_track`。这与"当前文件的元数据是什么"是正交问题。
- **v3 day-one 的妥协**：`Exif::has_embedded_media()` / `ExifIter::has_embedded_media()` / `TrackInfo::has_embedded_media()` 三个方法返回 bool，让用户至少能感知"还有数据没拿到"——避免 Live Photo HEIC 的内嵌 MOV 被静默丢失而用户无从察觉。具体内嵌流的提取留给 v3.x。

如果将来真的有需要同时返回多种元数据的场景，宁可在 v3.x 引入新方法（如 `read_all_metadata`），也不要在 `MediaKind` 上加 `Both`——后者会让所有现有调用方被迫升级。

### 8.7 为什么 feature 名是 `tokio` 而不是 `async`

v2 用 `async` feature 启用 tokio 依赖。v3 改名为 `tokio`。

- **`async` 暗示 runtime 无关**，但实际依赖只能是 tokio——名字与实质不符。
- **社区惯例倾向具体命名**：reqwest 的 `default-tls`/`rustls-tls`，redis-rs 的 `tokio-comp`/`async-std-comp`，sqlx 的 `runtime-tokio` 系列。
- **未来扩展友好**：若要支持 async-std/smol，可平行新增 `async-std` feature，而 `tokio` 名字仍准确。如果保留 `async` 这个名字，新增第二个 runtime 时会变得尴尬（`async` 该指哪个？）。
- **诚实优于优雅**：`tokio` 这个名字让用户立刻知道引入 tokio 依赖，避免"以为是 runtime 抽象"的误解。

未在 v3 中支持 async-std/smol 是因为：nom-exif 这种小库不值得做 runtime 抽象层；社区 async runtime 生态已经在向 tokio 收敛；如果有强烈需求，可在 v3.x 单独添加 feature，不破坏现有 `tokio` 用户。

### 8.8 为什么 feature 名是 `serde` 而不是 `json_dump`

v2 用 `json_dump` feature 启用 serde 依赖并派生 `Serialize`/`Deserialize`。v3 改名为 `serde`。

- **"dump" 是误导**：feature 本身不做 dump，它只是派生 serde traits。dump 行为发生在用户代码（`serde_json::to_string(&entry)?`）。
- **"json" 太窄**：派生的是 serde traits，不限于 JSON——下游用 `bincode` / `cbor` / `messagepack` / `yaml` / `toml` 都能用。`json` 字样会让用户以为只支持 JSON。
- **生态惯例几乎一边倒**：`chrono` / `uuid` / `bytes` / `url` / `indexmap` / `glam` / `regex` / `ndarray` 等主流库的 serde 集成 feature 都叫 `serde`。这是 Rust 生态的事实标准——下游看 Cargo.toml 第一眼就在找 `serde`。
- **诚实命名**：`serde` 这个名字精确反映 feature 的作用：派生 serde traits、引入 serde 依赖。

为何不抽象为 `to_json` / `to_yaml` 等具体方法：会强制库做格式选择并锁死下游，违背 serde 的设计初衷（trait-based 抽象，由用户决定具体格式 crate）。保留 `Serialize` / `Deserialize` 派生让下游灵活性最大。

---

## 9. 路线图（暂不展开）

本文档不包含分阶段实施计划。建议在设计经过 review 后另开文档讨论 PR 拆分、内部 trait 重构顺序、CI/测试策略等执行细节。

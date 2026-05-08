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
│   ├── mod.rs                Exif, ExifIter, ParsedExifEntry
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

/// 单个 Exif entry 的解析错误，用于 ExifIter 的 per-entry 错误
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum EntryError {
    #[error("entry size exceeds available data")]
    SizeOverflow,

    #[error("invalid entry data: {0}")]
    InvalidData(String),

    #[error("unsupported data format combination: {0}")]
    Unsupported(String),

    #[error("invalid datetime: {0}")]
    InvalidDateTime(String),
}
```

**关键变化：**
- 删除 `From<&str>` / `From<String> for Error`——这些 catch-all 实现是 v2 错误失控的根源。
- `Io(#[from] io::Error)` 真正承载 IO 错误，`?` 直接传播到正确变体。
- `EntryError` 改为公开 enum，不再包裹私有 `ParseEntryError`，下游可以 `match` 出具体原因。
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
#[non_exhaustive]
pub enum MediaKind {
    Image,    // 含 Exif 数据
    Video,    // 含 track 数据（含 audio-only 容器如 mka）
}

// 注：MediaKind 不提供 Both variant。
// HEIC 文件内嵌的 MOV（如 Apple Live Photos）属于 *embedded stream extraction*
// 范畴，未来通过独立 API（如 MediaSource::extract_embedded()）暴露，
// 而非通过此 enum 表达。

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

    /// 遍历所有 entry
    pub fn iter(&self) -> impl Iterator<Item = ExifEntry<'_>>;

    /// 遍历指定 IFD
    pub fn iter_ifd(&self, ifd: IfdIndex) -> impl Iterator<Item = ExifEntry<'_>>;

    pub fn ifds(&self) -> impl Iterator<Item = IfdIndex>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IfdIndex(pub usize);

impl IfdIndex {
    pub const MAIN: Self = IfdIndex(0);
    pub const THUMBNAIL: Self = IfdIndex(1);
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
- 增加 `iter()` / `iter_ifd()` / `ifds()`——v2 中转成 `Exif` 后就无法遍历，这是空白。
- `IfdIndex` newtype 替代魔术数字 `usize`，并提供 `MAIN`/`THUMBNAIL` 常量。
- `TagOrCode` 替代 v2 内部的 `ExifTagCode`（v2 是 `pub(crate)` 但其值通过 `tag()` / `tag_code()` 间接泄漏到公开 API）。

#### ExifIter（lazy）

```rust
pub struct ExifIter { ... }

impl ExifIter {
    pub fn rewind(&mut self);
    pub fn clone_rewound(&self) -> Self;
    pub fn parse_gps(&self) -> Result<Option<GPSInfo>>;
}

impl Iterator for ExifIter {
    type Item = ParsedExifEntry;
}

impl From<ExifIter> for Exif {
    fn from(iter: ExifIter) -> Self;
}

pub struct ParsedExifEntry {
    /* 不再有 take_value/take_result panic 路径 */
}

impl ParsedExifEntry {
    pub fn ifd(&self) -> IfdIndex;
    pub fn tag(&self) -> TagOrCode;       // 替代 tag()/tag_code() 双方法
    pub fn value(&self) -> Option<&EntryValue>;
    pub fn error(&self) -> Option<&EntryError>;
    pub fn result(&self) -> Result<&EntryValue, &EntryError>;
    pub fn into_result(self) -> Result<EntryValue, EntryError>;  // 替代 take_result
}
```

**关键变化：**
- 删除 `take_value` / `take_result`——这两个方法在 v2 里第二次调用会 panic，是隐藏陷阱。`into_result` 消费 self 即可避免该问题。
- `tag()` 返回 `TagOrCode` 替代 v2 里的 `Option<ExifTag>` + `tag_code()` 双方法，更直接。
- `clone_and_rewind` 改为更地道的 `clone_rewound`（动词→形容词，符合 Rust 惯例）。
- `parse_gps_info` 简化为 `parse_gps`。

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
    /// 返回携带时区信息的 DateTime；如果原始值是 NaiveDateTime，返回 None。
    pub fn as_datetime(&self) -> Option<DateTime<FixedOffset>>;

    /// 返回不带时区的 NaiveDateTime；DateTime 也会被转为 naive 形式。
    pub fn as_naive_datetime(&self) -> Option<NaiveDateTime>;

    // —— 弱类型转换（自动跨数值类型转换）——
    /// 任何整数类型 → i64（widening）
    pub fn try_as_integer(&self) -> Option<i64>;

    /// 任何 rational/float/integer → f64
    pub fn try_as_float(&self) -> Option<f64>;
}
```

**关键变化：**
- 补齐之前缺失的 `as_i64` / `as_f32` / `as_f64` / `as_u16_slice` / `as_u32_slice` / `as_undefined`。
- **`as_datetime` 直接返回 `DateTime<FixedOffset>`**——v2 中 `as_time_components` 返回 `(NaiveDateTime, Option<FixedOffset>)` 强迫用户做拼装，是文档示例都暴露的痛点。删除该方法。
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
    type Err = UnknownTagName;
    fn from_str(s: &str) -> Result<Self, Self::Err>;
}

impl fmt::Display for ExifTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

#[derive(Debug, thiserror::Error)]
#[error("unknown ExifTag name: {0}")]
pub struct UnknownTagName(pub String);
```

**关键变化：**
- 删除 `TryFrom<u16> for ExifTag`（v2 里返回 `crate::Error`——错误类型过重）。换成 `from_code(u16) -> Option<Self>`，语义更准确。
- 增加 `FromStr for ExifTag`，与 `TrackInfoTag` 对称。
- 删除 v2 内部的 `From<ExifTag> for &str`（功能与 `name()` / `Display` 重叠）。
- 用户访问未识别 tag 通过 `Exif::get_by_code(ifd, raw_code)` 或 `ParsedExifEntry::tag() == TagOrCode::Unknown(code)`。

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
    type Error = NegativeRational;
    fn try_from(value: IRational) -> Result<Self, Self::Error>;
}

#[derive(Debug, thiserror::Error)]
#[error("rational has negative numerator or denominator")]
pub struct NegativeRational;
```

**关键变化：**
- 隐藏字段，提供 `numerator()` / `denominator()`——v2 中 `r.0` / `r.1` 可读性很差。
- `to_f64()` 返回 `Option<f64>`，分母为 0 时返回 `None`（v2 中 `as_float()` 会返回 inf/nan）。
- `From<IRational> for URational` 改为 `TryFrom`，避免 silent truncation。

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

    /// 从十进制度数构造（带精度损失警告）
    pub fn from_decimal_degrees(degrees: f64) -> Self;
}

impl GPSInfo {
    pub fn latitude_decimal(&self) -> Option<f64>;
    pub fn longitude_decimal(&self) -> Option<f64>;
    pub fn altitude_meters(&self) -> Option<f64>;

    /// ISO 6709 字符串格式
    pub fn to_iso6709(&self) -> String;
}

impl FromStr for GPSInfo {
    type Err = Iso6709ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err>;
}

#[derive(Debug, thiserror::Error)]
#[error("invalid ISO 6709 coordinate: {message}")]
pub struct Iso6709ParseError { pub message: String }
```

**关键变化：**
- `latitude_ref: char` → `LatRef` enum，不再可能传入 `'X'` 这种非法值。
- `altitude_ref: u8 + altitude: URational` → `Altitude` enum，把 ref 与 value 绑成不可分离的整体。
- `speed_ref + speed` 同理合并为 `Option<Speed>`。
- `LatLng` 改为命名字段（`degrees`/`minutes`/`seconds`），不再是 `LatLng(URational, URational, URational)` 元组——可读性显著提升。
- 删除大量 `FromIterator`/`From<f64>` 实现（v2 有 `unwrap()` panic 风险）。仅保留 `from_decimal_degrees` 和 `new`。
- `format_iso6709` → `to_iso6709`（与 Rust 标准的 `to_*` 命名一致）。
- `InvalidISO6709Coord` → `Iso6709ParseError`（实现 `Error + Display`）。

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
/// 适合脚本与小工具；批量处理请改用 MediaParser 复用 buffer。
pub fn read_exif(path: impl AsRef<Path>) -> Result<Exif>;

/// 读取文件的 Exif 数据为 lazy 迭代器。
pub fn read_exif_iter(path: impl AsRef<Path>) -> Result<ExifIter>;

/// 读取视频/音频文件的 track 元数据。
pub fn read_track(path: impl AsRef<Path>) -> Result<TrackInfo>;

/// 自动判别 image / video 并返回统一结果。
pub fn read_metadata(path: impl AsRef<Path>) -> Result<Metadata>;

#[derive(Debug, Clone)]
pub enum Metadata {
    Exif(Exif),
    Track(TrackInfo),
}

// 注：Metadata 同样不提供 Both variant，理由与 MediaKind 一致——
// 当前 parser 的 MIME 探测层就是二选一，加 Both 会成为永远不返回的死代码 API。
// 真有"既需要 Exif 又需要 track"的场景（如 HEIC + 内嵌 MOV），
// 调用方应分别调用 read_exif 与 read_track 并各自处理 ExifNotFound/TrackNotFound 错误。

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
| `ParsedExifEntry` | struct | exif |
| `ExifTag` | enum | exif |
| `IfdIndex` | struct | exif |
| `TagOrCode` | enum | exif |
| `EntryValue` | enum | exif |
| `EntryError` | enum | exif |
| `URational` / `IRational` / `Rational<T>` | type/struct | exif |
| `NegativeRational` | error struct | exif |
| `UnknownTagName` | error struct | exif |
| `GPSInfo` | struct | exif::gps |
| `LatLng` | struct | exif::gps |
| `LatRef` / `LonRef` | enum | exif::gps |
| `Altitude` | enum | exif::gps |
| `Speed` / `SpeedUnit` | struct/enum | exif::gps |
| `Iso6709ParseError` | error struct | exif::gps |
| `TrackInfo` | struct | track |
| `TrackInfoTag` | enum | track |
| `Metadata` | enum | lib |
| `Error` | enum | error |
| `MalformedKind` | enum | error |
| `Result<T>` | type alias | error |
| `read_exif` / `read_exif_iter` / `read_track` / `read_metadata` | fn | lib |
| `read_*_async` | fn (feature) | lib |

**总计：约 33 个公开符号**（v2 约 16 个，但内部有不少 `pub(crate)` 类型间接通过 `pub` API 泄漏）。

### 4.2 模块 `nom_exif::prelude`

```rust
pub use crate::{
    EntryValue, Exif, ExifIter, ExifTag, GPSInfo,
    IfdIndex, MediaKind, MediaParser, MediaSource,
    Metadata, Result, TrackInfo, TrackInfoTag,
};
pub use crate::{read_exif, read_metadata, read_track};
```

prelude 只放最常用的；冷门类型（`Rational`、`LatLng`、各种 enum）保持 `nom_exif::Type` 显式导入。

---

## 5. v2 → v3 迁移指南

### 5.1 入口与解析

| v2 | v3 |
|----|-----|
| `MediaSource::file_path(p)` | `MediaSource::open(p)` 或 `read_exif(p)` |
| `MediaSource::tcp_stream(s)` | `MediaSource::unseekable(s)` |
| `ms.has_exif()` | `ms.kind() == MediaKind::Image` |
| `ms.has_track()` | `ms.kind() == MediaKind::Video` |
| `parser.parse::<_, _, ExifIter>(ms)` | `parser.parse_exif(ms)` |
| `parser.parse::<_, _, TrackInfo>(ms)` | `parser.parse_track(ms)` |
| `MediaSource<R, S>` 类型参数 | `MediaSource<R>`（S 已删除） |

### 5.2 错误处理

| v2 | v3 |
|----|-----|
| `Error::ParseFailed(box)` | 改用结构化变体 `Malformed { kind, message }` 或 `UnexpectedEof`、`UnsupportedFormat` |
| `Error::IOError(e)` | `Error::Io(e)`（名字精简） |
| `From<&str> for Error` | 已删除——内部错误请用具体变体 |
| `EntryError`（包私有 enum） | `EntryError`（公开 enum，可 match） |

### 5.3 EntryValue

| v2 | v3 |
|----|-----|
| `value.as_time_components()` | `value.as_datetime()` 或 `value.as_naive_datetime()` |
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
| `exif.get_by_ifd_tag_code(ifd, ExifTag::Make.code())` | `exif.get_in(IfdIndex(ifd), ExifTag::Make)` |
| 不能遍历 `Exif` | `exif.iter()` / `exif.iter_ifd(IfdIndex::MAIN)` |
| `entry.tag()` + `entry.tag_code()` | `entry.tag() -> TagOrCode` |
| `entry.take_value()` | `entry.into_result().ok()` 或先 clone |
| `entry.take_result()`（panic 风险） | `entry.into_result()`（消费 self） |
| `iter.clone_and_rewind()` | `iter.clone_rewound()` 或 `let mut x = iter.clone(); x.rewind();` |
| `iter.parse_gps_info()` | `iter.parse_gps()` |

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
2. ~~**`Metadata::Both` variant？**~~ **已决议**：不加。`MediaKind` 与 `Metadata` 都保持二选一。HEIC 内嵌 MOV 等场景属于 *embedded stream extraction* 范畴，未来通过独立 API 暴露。详见 §8.6。
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

- **当前 MIME 探测层就是二选一**：`MediaMime::Image | Video`，HEIC 会被归为 `Image`，parser 不会去解析其内嵌的 MOV 流。即使 enum 加 `Both`，目前没有任何代码路径会构造它——这是死代码。
- **加 `Both` 让所有调用方付出代价**：`match` 需要多一条分支，但 99% 的文件只有一种元数据。便利性反而下降。
- **正确的解法是 *embedded stream extraction***：未来通过独立 API（如 `MediaSource::extract_embedded() -> impl Iterator<Item = MediaSource>`）暴露内嵌流，由用户对每个流单独 `parse_exif` / `parse_track`。这与"当前文件的元数据是什么"是正交问题。

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

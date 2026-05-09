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
}
// Format-level granularity (Heif / Cr3Container / Raf) was removed in favor
// of structural-only categories — those families are all built on top of
// IsoBmffBox / TiffHeader / JpegSegment, and the `message` string carries
// the format prefix (e.g. "cr3: no CMT data found").

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
    mime: MediaMime,         // pub(crate)，不在公开 API 暴露
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
}

impl<R> MediaSource<R> {
    pub fn kind(&self) -> MediaKind;
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
// 场景属于 *embedded track extraction* 范畴（见 `Exif::has_embedded_track()`、
// 未来的 `MediaSource::extract_embedded()`），不通过此 enum 表达。
//
// 注：`MediaMime`（具体格式 enum：Jpeg/Heic/Heif/.../Mp4/Mov/...）保持
// pub(crate)，不在 v3 公开 API 暴露：
// 一、enum 的具体 variant 集合还不稳定（新格式可能频繁加入，部分边界
//     case 如 HEIC vs HEIF 仍在调整），过早暴露会带来兼容性负担；
// 二、用户的实际分发需求基本被 `MediaKind` 覆盖；剩下的"知道是 JPEG 还是
//     HEIC"的诊断/日志诉求未必要承担一个 13 variant 的稳定 API 表面。
// 待格式集合稳定、并出现确凿的下游需求后，可在 v3.x 增量暴露
// （`MediaSource::mime()` / `MediaMime` enum）。
```

**变化点：**
- 删除 `MediaSource::tcp_stream`——它只是 `unseekable` 的别名，无独特行为。
- 同理删除 `MediaSource::from_file`——它只是 `seekable` 的别名（`File: Read+Seek`），无 `File` 特有逻辑。已经有了路径参数的 `open(path)` 提供独有的"打开+解析"语义；想从已打开的 `File` 构造的用户写 `MediaSource::seekable(file)` 即可。这条与 `tcp_stream` 的删除同源（§8.2）：API 表面只保留有独立行为的入口。
- 删除 `has_exif()` / `has_track()`，统一为 `kind()`，避免"两个 bool 互斥"的隐含约定。
- `MediaSource::open(path)` 取代 v2 的 `file_path`——名字更短更地道。
- `SkipStrategy` 是私有的（不出现在 `pub use` 中），构造时由 `seekable` /
  `unseekable` 二选一确定，**运行时不会回退**：seek 失败直接返回 `Error::Io`。
  v2 通过 `Skip` trait + `bool` 返回值允许"先尝试 seek 再 fallback 到 read"
  的隐式回退；v3 移除该回退（理由见 §7.3：静默回退会掩盖被截断的 file
  handle 等真实问题，且性能特征会突然劣化让用户难以诊断）。下游若需要双
  策略，请显式构造两个 `MediaSource`。

#### MediaParser

```rust
pub struct MediaParser { /* 内部单槽 buffer + 共享态，对外不可见 */ }

impl MediaParser {
    /// 零分配构造：不会预分配任何 parse buffer，第一次 parse 时按需申请。
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
- **内部 buffer 模型简化为单槽 + `bytes::Bytes::try_into_mut` 回收**（详见 §6 与 §8.9）：
  v2 的多槽 `Buffers` 池被一个 `Option<Bytes>` 取代——`MediaParser` 的方法都是 `&mut self`，
  任意时刻只有一个 active buffer，多槽是过度设计；用 `Bytes` 的 refcount 语义就能精确判断
  上一轮 `ExifIter` 是否已 drop（drop 后 refcount 降到 1，下次 parse 直接零拷贝复用同一块 alloc）。
  对外行为不变，但 `MediaParser::new()` 不再预分配（v2 是 2 × 4 KB = 8 KB 上来就分配）。

#### 内存数据源（v3 内提供）

针对 "已经把整段数据放在内存里" 的常见场景（WASM、移动端、HTTP 代理服务等），v3 提供
zero-copy 内存输入：

```rust
impl MediaSource<()> {
    /// 从内存中的字节构造 MediaSource。`Bytes`、`Vec<u8>`、`&'static [u8]` 都能转入。
    /// 解析过程**不会拷贝**输入字节——`ExifIter` 直接借用同一块 `Bytes`。
    pub fn from_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Self>;
}
```

**设计要点：**
- 公开类型只增加这一个构造器，没有引入新的 source 类型——内存模式由 `MediaSource` 内部
  状态区分（`reader` 槽位为 `()`，整段数据驻留在 `Bytes` 里），下游不感知。
- 真 zero-copy：`fill_buf` / `clear_and_skip` 内存路径变成 no-op / `position += n`，
  share buf 直接 `Bytes::clone()` 给 `ExifIter`，连内部单槽 cache 都不用走。
- `bytes::Bytes` 已是 nom-exif 的硬依赖（用于 ebml VInt 解析），暴露它不会引入新 transitive
  dep。
- 对 streaming 路径完全无侵入——是新增并行路径，不动旧代码。
- 之所以接 `Bytes` 而非 `&[u8]`：要求 owning 输入避免在 API 边界引入生命周期参数；
  `Bytes::from(&'static [u8])`、`Bytes::from(Vec<u8>)`、`bytes::Bytes::from_owner(...)` 已经
  覆盖绝大多数构造方式，且能与 hyper/axum 等生态的原生载荷类型直通。

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

    /// 此文件是否嵌入了一段未被本次解析处理的媒体 track（例如 Pixel/Google
    /// Motion Photo JPEG 末尾追加的 MP4）。基于 parse_exif 期间观察到的
    /// 内容信号判定（如 `GCamera:MotionPhoto="1"` XMP 属性），不做 MIME
    /// 级猜测。`true` 时调用 `parse_track` 即可拿到嵌入 track 的元数据。
    /// 见 §8.6。
    pub fn has_embedded_track(&self) -> bool;

    /// Deprecated（3.1.0）：等同于 `has_embedded_track()`。
    #[deprecated]
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

    /// 与 `Exif::has_embedded_track()` 同义；header 解析后即可返回，
    /// 不需要驱动迭代器。
    pub fn has_embedded_track(&self) -> bool;

    /// Deprecated（3.1.0）：等同于 `has_embedded_track()`。
    #[deprecated]
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
- `ExifEntry`（eager 视图）使用 plain pub fields：三个字段彼此独立、无
  内部不变量，pub 字段是符合 Rust 惯例的最简形式（参见 `std::ops::Range`
  这类纯数据类型）。`ExifIterEntry`（lazy yield）则使用私有字段 + getters：
  类型本身带有 *value xor error* 不变量，pub 字段会让外部代码构造出
  `value=Some, error=Some` 这类无意义状态；用 getters 把 `result()` /
  `into_result()` 暴露为主 API 既保持不变量又给出最自然的使用方式。一句话：
  **是否有字段间不变量决定该用 pub fields 还是 getters**——这是 v3 整篇文档
  里的统一原则。

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
    // —— 标量访问器 ——
    // 不提供 as_i8 / as_i16 / as_f32：这些 TIFF format 在现代 EXIF 中几乎
    // 不出现，单独暴露收益不大；若真遇到这类 tag，直接 match `EntryValue`
    // 变体，或用下方的 `try_as_integer` / `try_as_float` 做宽化转换即可。
    pub fn as_str(&self) -> Option<&str>;
    pub fn as_u8(&self) -> Option<u8>;
    pub fn as_u16(&self) -> Option<u16>;
    pub fn as_u32(&self) -> Option<u32>;
    pub fn as_u64(&self) -> Option<u64>;
    pub fn as_i32(&self) -> Option<i32>;
    pub fn as_i64(&self) -> Option<i64>;       // 新增
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
    /// （`NaiveDateTime`）。返回 `ExifDateTime` 包装，让类型如实反映这一点。
    pub fn as_datetime(&self) -> Option<ExifDateTime>;

    // —— 弱类型转换（自动跨数值类型转换）——
    /// 任何整数类型 → i64（widening）
    pub fn try_as_integer(&self) -> Option<i64>;

    /// 任何 rational/float/integer → f64
    pub fn try_as_float(&self) -> Option<f64>;
}

/// EXIF datetime 的两种形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExifDateTime {
    /// 原始值携带时区（如 `OffsetTimeOriginal` 拼装而来）
    Aware(DateTime<FixedOffset>),
    /// 原始值不带时区（如裸的 `DateTime` tag）
    Naive(NaiveDateTime),
}

impl ExifDateTime {
    /// 仅当原值带时区时返回；否则 None。
    pub fn aware(&self) -> Option<DateTime<FixedOffset>>;

    /// 总是返回 NaiveDateTime；带时区的会被剥离时区。
    pub fn into_naive(self) -> NaiveDateTime;

    /// 不带时区时套用 fallback 偏移；带时区时保留原偏移。
    pub fn or_offset(self, fallback: FixedOffset) -> DateTime<FixedOffset>;
}
```

**关键变化：**
- 补齐之前缺失的 `as_i64` / `as_f64` / `as_u16_slice` / `as_u32_slice` / `as_undefined`；同时刻意不提供 `as_i8` / `as_i16` / `as_f32`（现代 EXIF 中几近不出现，遇到时 match 变体或走 `try_as_*` 宽化）。
- **单一 `as_datetime() -> Option<ExifDateTime>`**：v2 的 `as_time_components` 返回 `(NaiveDateTime, Option<FixedOffset>)` 强迫用户做拼装。原 v3 草案拆成 `as_datetime` + `as_naive_datetime` 两个方法又会让"任何 datetime"的取值要 fallback；最终采用 `ExifDateTime` enum，类型如实反映"EXIF datetime 可能带或不带时区"，三类调用方各取所需：`val.as_datetime()?.aware()`、`?.into_naive()`、`?.or_offset(fallback)`。
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
    // 不提供 into_parts：与下方 `From<Rational<T>> for (T, T)` 重复，留 From 即可。
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

    /// Deprecated（3.1.0）：3.0.0 预留用于"track 源里又嵌一个 track"
    /// 检测，但从未真正实现，永远返回 false。3.1.0 不再保留对称的
    /// `has_embedded_track`，只留这个 no-op 占位以保持源兼容；如果
    /// 未来出现真实用例再重新引入。
    #[deprecated]
    pub fn has_embedded_media(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum TrackInfoTag {
    Make, Model, Software, CreateDate,
    DurationMs, Width, Height,
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
/// 不在内部套 `BufReader`——`MediaParser` 的 `fill_buf` 已经按
/// `MIN_GROW_SIZE`（16 KB）批量读取，再加一层 8 KB 默认缓冲只会增加
/// 一次 memcpy 而 syscall 节省微乎其微。批量场景仍建议直接用
/// `MediaParser` 复用 buffer。
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

// 注：Metadata 同样不提供 Both variant（无 #[non_exhaustive]，封闭 enum），
// 理由与 MediaKind 一致——当前 parser 的 MIME 探测层就是二选一，加 Both
// 会成为永远不返回的死代码 API。
//
// 用户判断"图片里是否还嵌了一段 track"通过 `Exif::has_embedded_track()`
// （Pixel Motion Photo 等）。真有"既需要 Exif 又需要 track"的场景，
// 调用方应分别调用 read_exif 与 read_track 并各自处理 ExifNotFound /
// TrackNotFound 错误。

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
| `Exif` | struct | exif |
| `ExifIter` | struct | exif |
| `ExifEntry<'a>` | struct | exif |
| `ExifIterEntry` | struct | exif |
| `ExifTag` | enum | exif |
| `IfdIndex` | struct | exif |
| `TagOrCode` | enum | exif |
| `EntryValue` | enum | exif |
| `ExifDateTime` | enum | exif |
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

**总计：约 29 个公开符号**（v2 约 16 个，但内部有不少 `pub(crate)` 类型间接通过 `pub` API 泄漏）。`UnknownTagName` / `Iso6709ParseError` / `NegativeRational` 三个一字段错误结构合并为统一的 `ConvertError` enum；`MediaMime` 与 `MediaSource::mime()` 在 v3 保持 `pub(crate)`（理由见 §3.3）。

### 4.2 模块 `nom_exif::prelude`

```rust
pub use crate::{
    EntryValue, Error, Exif, ExifIter, ExifTag, GPSInfo,
    IfdIndex, MalformedKind, MediaKind, MediaParser, MediaSource,
    Metadata, Result, TrackInfo, TrackInfoTag,
};
pub use crate::{read_exif, read_metadata, read_track};
```

prelude 包含 `Error` 与 `MalformedKind`：用户匹配错误时不必再显式导入。冷门类型（`Rational`、`LatLng`、`ConvertError`、`ExifDateTime` 等）保持 `nom_exif::Type` 显式导入。

---

## 5. v2 → v3 迁移指南

完整的、面向用户的迁移指南已抽离到 [`docs/MIGRATION.md`](MIGRATION.md)，
其中每一行都由 `tests/migration_guide.rs` 实测覆盖。本文档保留为内部
设计契约，专注架构决策与权衡（§1-§4、§6 起及之后），不再重复迁移表。

Feature 改名（`async` → `tokio`、`json_dump` → `serde`）的设计动机见
§8.7 和 §8.8。

---

## 6. 内部架构影响（非 API 但需配套）

虽然本文档聚焦公开接口，以下内部变化是 v3 的必要配套：

1. **去重 sync/async 解析逻辑**：v2 中 `parser.rs` 与 `parser_async.rs` 大量重复。v3 通过私有 `BufParser` / `AsyncBufParser` trait（共享 `parse_loop_step` / `clear_and_skip_decide` 等纯函数）合并。
2. **`PartialVec` 整体删除，内部 byte-view 类型统一为 `bytes::Bytes`**（P4.5）：`PartialVec` / `AssociatedInput` 这两个 `pub(crate)` 类型连同 `src/partial_vec.rs` 整个文件删除。`IfdIter::input` / `ExifIter::input` 都直接持有 `Bytes`；`partial(&[u8])` 的所有调用点收敛到 `Bytes::slice` / `Bytes::clone`（详见 §8.10）。CR3 多块路径调整为 *上游一次切片* 模式：`parse_cr3_exif_iter` 在 `share_buf` 拿到全 buffer 后立刻把每个 CMT 块预切成独立 `Bytes`，下游 iterator 不再需要持有"全 alloc 句柄 + range"的二元组。公开 API 零影响。`Bytes` 已是硬依赖，切换后享受 `try_into_mut` / `slice_ref` / `from_static` 等成熟 API，并为 §3.3 的内存数据源（P7）铺路。
3. **删除多槽 buffer 池**（P4.5）：`src/buffer.rs` 里的 `Buffers` 三态结构（pool / shared / acquired）整体删除（约 -291 LOC，含相关单元测试）。`MediaParser` 单线程使用 `&mut self`，任意时刻只有一个 active buffer，多槽是过度设计。改为单 `Option<Bytes>` cache + `Bytes::try_into_mut` 回收，详见 §8.9。
4. **`ParsingError` / `ParsingErrorState`**：完全私有，不再在公开 trait 边界出现。
5. **`Mime`**：改名 `MediaMime`，但保持 `pub(crate)`（v3 不在公开 API 暴露具体格式 enum；待格式集合稳定后在 v3.x 增量暴露）。
6. **`TiffHeader`**：保持 `pub(crate)`。
7. **MSRV**：建议从 1.80 升到 1.83+（用 `expect` lint、`Option::is_some_and` 等）。
8. **内存模式 parse 路径**（P7）：当 `MediaSource` 持有的是已就位的 `Bytes` 而非 reader 时，parser 走一条 no-op `fill_buf` + `position += n` 跳跃 + 直接 `Bytes::clone` share 的并行路径。对 streaming 路径无侵入。

---

## 7. 开放问题 / 已决议项

以下问题在 2026-05-08 的设计 review 中均已决议。后续如发现实现障碍或新证据，可重新讨论。

1. ~~**`Exif::iter()` 的 item 类型？**~~ **已决议**：保持 `ExifEntry<'a>` struct（零拷贝引用），不退化为元组。理由：未来扩展（增加 IFD 内偏移量、原始字节等字段）不需要破坏 API；用户用 `entry.tag` / `entry.value` 比 `entry.0` / `entry.1` 可读性更好。
2. ~~**`Metadata::Both` variant？**~~ **已决议**：不加。`MediaKind` 与 `Metadata` 都保持二选一（封闭 enum，无 `#[non_exhaustive]`）。Pixel Motion Photo 等"图片里嵌一段 track"的场景属于 *embedded track extraction* 范畴：当前通过 `Exif::has_embedded_track()` / `ExifIter::has_embedded_track()`（基于内容检测置位）+ `parse_track` 在 image MIME 上的 polymorphic 分支拿到嵌入 track。3.0.0 的 `has_embedded_media()` 是已 deprecated 的别名。详见 §8.6。
3. ~~**`MediaSource` 的 skip fallback？**~~ **已决议**：`seek` 失败时返回 `Error::Io(...)`，不静默回退到 `Read`。理由：静默回退会掩盖真实问题（例如调用方传入了被截断的 file handle），且性能特征会突然劣化让用户难以诊断。
4. ~~**`async` feature 命名 / 多 runtime 支持？**~~ **已决议**：feature 改名为 `tokio`（不再用误导性的 `async`）。v3 仅支持 tokio，未来如需 async-std/smol 平行新增对应 feature。详见 §8.7。
5. ~~**`json_dump` feature？**~~ **已决议**：feature 改名为 `serde`（与生态惯例对齐）。仍直接派生 `Serialize` / `Deserialize`，不抽象为 `to_json` 方法——保持下游灵活性最大（任何 serde-compatible 格式都能用，不锁死 JSON）。详见 §8.8。
6. ~~**MakerNote per-vendor 解析？**~~ **已决议**：不在 v3 范围。v3 仍仅返回 `EntryValue::Undefined(Vec<u8>)`，下游可自行解析或使用专门的 makernote crate。理由：每家厂商的 makernote 格式不公开且变化频繁，做不好会成长期维护负担；v3 的核心目标是收敛 API，而非扩功能。可作为 v3.x 增量特性（如新增 `makernote` feature）。
7. ~~**写入支持？**~~ **已决议**：v3 仍是只读。写入需要重新设计整个数据流（解析 → 修改 → 重写），与 v3 的"读取 API 收敛"目标正交。作为 v4 议题。
8. ~~**`MediaParser` 内置 buffer 是否对外可控？**~~ **已决议**：保持内置、不外部注入、不加 on/off 开关。同时简化为单槽 `Option<Bytes>` cache + `Bytes::try_into_mut` 回收，删除 v2 的多槽 `Buffers` 池。理由：`MediaParser` 单线程使用 `&mut self`，多槽是过度设计；外部注入会把 `Bytes` 引用计数语义抬上公开 API，违背"收敛"目标；当前没有具体用户痛点支撑增加旋钮。`MediaParser::new()` 顺带改成零分配（v2 上来就 `2 × 4 KB`）。详见 §8.9。落地于 P4.5。
9. ~~**内存数据源（`Vec<u8>` / `&[u8]` / `Bytes`）支持？**~~ **已决议**：v3 内提供 zero-copy 内存源 `MediaSource::from_bytes(impl Into<bytes::Bytes>)`。理由：`bytes::Bytes` 已是硬依赖，P4.5 完成 `PartialVec` 删除（决议 8 + §8.10）后内部 byte-view 已统一到 `Bytes`，内存路径几乎是平凡的——`fill_buf` 变 no-op、`clear_and_skip` 变 `position += n`、share 直接 `Bytes::clone`。增量公开表面只多一个构造器，对 streaming 路径零侵入；面向 WASM / 移动端 / HTTP 代理服务等"字节已在内存里"的场景。落地于 P7（独立 phase，依赖 P4.5）。如 v3.0.0 cutover 范围紧张，可推迟到 v3.1 而不破坏其他承诺。

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

- **当前 MIME 探测层就是二选一**：`MediaKind::Image | Track`（内部根据格式 sniff 分类，具体格式 enum 当前 pub(crate)），HEIC 会被归为 `Image`，parser 不会去解析其内嵌的 MOV 流。即使 enum 加 `Both`，目前没有任何代码路径会构造它——这是死代码。
- **加 `Both` 让所有调用方付出代价**：`match` 需要多一条分支，但 99% 的文件只有一种元数据。便利性反而下降。
- **正确的解法是 *embedded track extraction***：未来通过独立 API（如 `MediaSource::extract_embedded() -> impl Iterator<Item = MediaSource>`）暴露内嵌流，由用户对每个流单独 `parse_exif` / `parse_track`。这与"当前文件的元数据是什么"是正交问题。
- **v3.1 的取舍**：`Exif::has_embedded_track()` / `ExifIter::has_embedded_track()` 两个方法返回 bool，告诉用户"图片里还嵌了一段 track 待提取"。`parse_track` 对 image MIME 加 polymorphic 分支，命中即抽出来。
  > **3.1 升级历程**：(a) v3.0.0 这两个方法叫 `has_embedded_media()`，3.1.0 改名为 `has_embedded_track()`，旧名作为 `#[deprecated]` 别名保留；(b) 实现从"MIME 级猜测"升级为"内容检测"——`parse_exif` 走 JPEG 路径时扫描 XMP，看见 `GCamera:MotionPhoto="1"` 才置位（覆盖 Pixel/Google Motion Photo 和走 Adobe Container directory 的 Samsung Galaxy Motion Photo），其他格式默认 false；(c) `parse_track` 对 image MIME 不再立即 `TrackNotFound`：JPEG 走 polymorphic 路径，扫到 Motion Photo trailer 就解析其内嵌 MP4 返回 `TrackInfo`。仅依赖 `MotionPhoto_Data` trailer 标记的老 Samsung 文件、HEIC + `moov` 留 v3.x。`TrackInfo::has_embedded_media` 在 3.0.0 是预留位（永远 false，从未实装），3.1 维持 deprecated no-op，等真实用例再考虑重新引入。

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

### 8.9 为什么 buffer 池从多槽改成单槽 + `Bytes::try_into_mut`

v2 的 `src/buffer.rs::Buffers` 是一个手写的三态 pool：`pool`（空闲、可直接 acquire）、`shared`（已被 `ExifIter` 借走的 `Arc<Vec<u8>>`、待 refcount 降回 1 后回收）、`acquired`（计数器）。`MediaParser::default()` 还会预分配 `INIT_POOLED_BUF = 2` 块 4 KB buffer，意图是 "首次 parse 不付分配成本"。

v3 改成：`MediaParser` 只持有 `Option<Bytes>` 一个槽位，acquire 时若有缓存则尝试 `Bytes::try_into_mut`——成功（refcount==1）即零拷贝复用同一块 alloc，失败（上一轮 ExifIter 仍存活）则丢弃缓存重新分配。

**为什么多槽是过度设计：**
- **`MediaParser` 的解析方法都是 `&mut self`**：单线程独占，任意时刻只有一块 active buffer。pool > 1 的容量永远是死代码。
- **共享态 / pool 二分本质上重新发明了 `Arc::try_unwrap` 的语义**：`Bytes::try_into_mut`（自 bytes 1.4 起稳定）就是这件事的一等公民 API，且是 zero-copy（`Vec::from(BytesMut)` 在 alloc 唯一时不复制）。
- **预分配的 2 × 4 KB 是负优化**：one-shot 路径（`read_exif(path)` 内部新建一次性 parser）每次都浪费 8 KB；复用 parser 的路径首次 parse 多付一次 4 KB 分配（可忽略）。改成 lazy 净收益。
- **`MAX_POOLED_BUF = 8` 限额下的 "尾巴" 也没价值**：如果用户同时持有 8 个 `ExifIter`，pool 也没法回收任何一块；下一次 parse 只能新分配。pool 在这场景里只是延迟 GC 而已。

**收益（落地于 P4.5）：**
- `src/buffer.rs` 整体删除（约 -291 LOC，含 2 个 pool 单元测试）。
- `MediaParser::new()` 零分配。
- 复用语义更精确：单槽 + `try_into_mut` 等价于 "上一轮 ExifIter 已 drop ⟺ 复用同一块 alloc"，是否复用对用户而言是明确可控的（drop ExifIter / 转 Exif 即可触发回收）。
- 对外行为不变，无 API 影响。

**代价：**
- 高级用户若想强制让多个 long-lived `ExifIter` 共享同一块底层 alloc——做不到（单槽下每个并存的 ExifIter 持有独立 alloc）。但 v2 的多槽 pool 也做不到这件事，且没有任何用户提过这种诉求。是真正的零代价决策。

### 8.10 为什么彻底删 `PartialVec`，而不是只把它的内部存储换成 `Bytes`

最初的 P4.5 草案是"`PartialVec` 外壳保留，把 `data: Arc<Vec<u8>>` 换成 `data: Bytes`"。这个方案能用，但留了一个本可消解的中间类型——v3 的核心精神是收敛抽象，最终选择把 `PartialVec` / `AssociatedInput` 整个删除。

**触发这个决策的复盘：** 当时认为"必须保留 `PartialVec`，因为 CR3 多块路径需要 `(full_alloc, view_range)` 二元组语义"。重新追代码后这个假设站不住。`PartialVec::partial(&[u8])` 在代码里只有 4 个调用点：

| 位置 | 用法 | `Bytes` 替换 |
|---|---|---|
| `exif_iter.rs:210` | `input.partial(&input[..])` | `input.clone()` |
| `exif_iter.rs:619` | `input.partial(&input[offset..])` | `input.slice(offset..)` |
| `exif_iter.rs:740` | `input.partial(&input[..])` | `input.clone()` |
| `exif_iter.rs:401` | CR3 块构造，需要"反查全 alloc" | 见下 |

前三处都在当前视图范围内，`Bytes::clone` / `Bytes::slice` 一一对应。**唯一真正需要"突破当前视图"的是 CR3 多块路径**——但这个需求是 v2 实现细节带来的人为依赖，并非本质需求：

- v2 流程：`share_buf` 给 `ExifIter` 一个 `PartialVec(full_alloc, primary_range)`；`ExifIter` 持有 full alloc 句柄，后续按需切出 CMT2/CMT3 视图。
- v3 流程：`parse_cr3_exif_iter` 在拿到 full `Bytes` 的那一刻就把 *所有* CMT 块（CMT1 主块 + CMT2/CMT3 附加块）一次切完，下游 `ExifIter` 只持有已切好的 `Bytes` 列表。`add_tiff_block` 签名从 `(block_id, Range<usize>, Option<TiffHeader>)` 变成 `(block_id, Bytes, Option<TiffHeader>)`。

**为什么 *上游一次切完* 是更好的形态：**
- **不变量更简单**：调用方手里的 `Bytes` 直接就是它要消费的字节范围；不需要"我现在的视图是什么 vs 我能反查到什么"这种二元思维。
- **`partial_vec.rs` 整个删除**（~100 LOC + From impls + 单元测试）：`Bytes` 已经把"owning byte view + cheap clone + slice/slice_ref"做到比手写抽象更干净的程度，留一个薄壳没价值。
- **P7（内存数据源）天然受益**：内存模式下 share_buf 直接是 `Bytes::clone`，下游一致地拿到 `Bytes`——streaming 路径与内存路径的代码完全对称。
- **每个 callsite 都是机械替换**：`PartialVec` / `AssociatedInput` 都是 `pub(crate)`，外部 API 零影响；P4.5 的 diff 大约 ~150–200 LOC，但全是覆盖率高的路径（`exif_iter.rs::IfdIter` / `ExifIter` / `parse_cr3_exif_iter`），每一步都能独立 commit + `cargo test --all-features` 验证。

**代价：** P4.5 工作量从 ~0.5 天涨到 ~1 天；blast radius 从单文件扩到 4 个文件。但终态没有遗留的中间类型，P5（ExifIter / Exif 双 API）的起点是干净的。

---

## 9. 路线图（暂不展开）

本文档不包含分阶段实施计划。建议在设计经过 review 后另开文档讨论 PR 拆分、内部 trait 重构顺序、CI/测试策略等执行细节。

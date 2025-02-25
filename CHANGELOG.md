# Changelog

## nom-exif v2.4.3

### Fixed

- Ignore undetermined language flag (0x55c4) when parsing `TrackInfoTag::Author` info #36

## nom-exif v2.4.2

### Fixed

- Fix: panic: skip a large number of bytes #28 #26
- Fix: panic when parsing moov/trak #29

## nom-exif v2.4.1

### Fixed

- Fix: Panic for Nikon D200 raw NEF file (parse_ifd_entry_header) #34

## nom-exif v2.4.0

### Added

- Add `TrackInfoTag::Author` #36
  - Parse `udta/auth` or `com.apple.quicktime.author` info from `mp4`/`mov` files

## nom-exif v2.3.1

[v2.3.0..v2.3.1](https://github.com/mindeng/nom-exif/compare/v2.3.0..v2.3.1)

### Fixed

- parse GPS strings correctly #35

## nom-exif v2.3.0

[v2.2.1..v2.3.0](https://github.com/mindeng/nom-exif/compare/v2.2.1..v2.3.0)

### Added

- `EntryValue::U8Array`

### Fixed

- Doesn't recognize DateTimeOriginal tag in file. #33
- Update to avoid Rust 1.83 lints #30
- println! prints lines to output #27
- range start index panic in src/exif/exif_iter.rs #25
- Panic range end index 131151 out of range for slice of length 129 #24
- Memory allocation failed when decoding invalid file #22
- assertion failed: data.len() >= 6 when checking broken exif file #21
- Panic depth shouldn't be greater than 1 #20
- freeze when checking broken file #19

## nom-exif v2.2.1

[v2.1.1..v2.2.1](https://github.com/mindeng/nom-exif/compare/v2.1.1..v2.2.1)

### Added

- Added support for RAF (Fujifilm RAW) file type.

## nom-exif v2.1.1

[v2.1.0..v2.1.1](https://github.com/mindeng/nom-exif/compare/v2.1.0..v2.1.1)

### Fixed

- Fix endless loop caused by some broken images.

## nom-exif v2.1.0

[v2.0.2..v2.1.0](https://github.com/mindeng/nom-exif/compare/v2.0.2..v2.1.0)

### Added

- Type alias: `IRational`
- Supports 32-bit target platforms, e.g.: `armv7-linux-androideabi`

### Fixed

- Fix compiling errors on 32-bit target platforms

## nom-exif v2.0.2

[v2.0.0..v2.0.2](https://github.com/mindeng/nom-exif/compare/v2.0.0..v2.0.2)

### Changed

- Deprecated
  - `parse_mov_metadata`: Please use `MediaParser` instead.

## nom-exif v2.0.0

[v1.5.2..v2.0.0](https://github.com/mindeng/nom-exif/compare/v1.5.2..v2.0.0)

### Added

- Support more file types
  - `*.tiff`
  - `*.webm`
  - `*.mkv`, `*.mka`
  - `*.3gp`

- rexiftool
  - Add `--debug` command line parameter for printing and saving debug logs

- Structs
  - `MediaSource`
  - `MediaParser`
  - `AsyncMediaSource`
  - `AsyncMediaParser`
  - `TrackInfo`
  
- Enums
  - `TrackInfoTag`
  
- Type Aliases
  - `URational`
  
### Changed

- Deprecated
  - `parse_exif`     : Please use `MediaParser` instead.
  - `parse_exif_async` : Please use `MediaParser` instead.
  - `parse_heif_exif` : Please use `MediaParser` instead.
  - `parse_jpeg_exif` : Please use `MediaParser` instead.
  - `parse_metadata` : Please use `MediaParser` instead.
  - `FileFormat`     : Please use `MediaSource` instead.

## nom-exif v1.5.2

[v1.5.1..v1.5.2](https://github.com/mindeng/nom-exif/compare/v1.5.1..v1.5.2)

### Fixed

- Bug fixed: "Box is too big" error when parsing some mov/mp4 files

  No need to limit box body size when parsing/traveling box headers, only need
  to do that limitation when parsing box body (this restriction is necessary
  for the robustness of the program). Additionally, I also changed the size
  limit on the box body to a more reasonable value.

## nom-exif v1.5.1

[v1.5.0..v1.5.1](https://github.com/mindeng/nom-exif/compare/v1.5.0..v1.5.1)

### Added

- `ParsedExifEntry`

### Changed

- `ExifTag::Unknown`

## nom-exif v1.5.0

[v1.4.1..v1.5.0](https://github.com/mindeng/nom-exif/compare/v1.4.1..v1.5.0)

### Added

- `parse_exif`
- `parse_exif_async`
- `ExifIter`
- `GPSInfo`
- `LatLng`
- `FileFormat`
- `Exif::get`
- `Exif::get_by_tag_code`
- `EntryValue::URationalArray`
- `EntryValue::IRationalArray`
- `Error::InvalidEntry`
- `Error::EntryHasBeenTaken`

### Changed

- `Exif::get_values` deprecated
- `Exif::get_value` deprecated
- `Exif::get_value_by_tag_code` deprecated
- `Error::NotFound` deprecated

## nom-exif v1.4.1

[v1.4.0..v1.4.1](https://github.com/mindeng/nom-exif/compare/v1.4.0..v1.4.1)

### Performance Improved

- Avoid data copying when extracting moov body.

### Added

- impl `Send` + `Sync` for `Exif`, so we can use it in multi-thread environment

## nom-exif v1.4.0

[v1.3.0..v1.4.0](https://github.com/mindeng/nom-exif/compare/v1.3.0..v1.4.0)

### Performance Improved

- Avoid data copying during parsing IFD entries.

## nom-exif v1.3.0

[v1.2.6..v1.3.0](https://github.com/mindeng/nom-exif/compare/v1.2.6..v1.3.0)

### Changed

- Introduce tracing, and replace printing with tracing.

## nom-exif v1.2.6

[v1.2.5..v1.2.6](https://github.com/mindeng/nom-exif/compare/v1.2.5..v1.2.6)

### Fixed

- Bug fixed: [A broken JPEG file - Library cannot read it, that exiftool reads
  properly #2](https://github.com/mindeng/nom-exif/issues/2)
- Bug fixed: [Another Unsupported MP4 file
  #7](https://github.com/mindeng/nom-exif/issues/7#issuecomment-2226853761)

### Internal

- Remove redundant `fn open_sample` definitions in test cases.
- Use `read_sample` instead of `open_sample` when possible.

## nom-exif v1.2.5

[v1.2.4..v1.2.5](https://github.com/mindeng/nom-exif/compare/v1.2.4..v1.2.5)

### Fixed

- Bug fixed: [Unsupported mov file?
  #7](https://github.com/mindeng/nom-exif/issues/7)

### Internal

- Change `travel_while` to return a result of optional `BoxHolder`, so we can
  distinguish whether it is a parsing error or just not found.

## nom-exif v1.2.4

[8c00f1b..v1.2.4](https://github.com/mindeng/nom-exif/compare/8c00f1b..v1.2.4)

### Improved

- **Compatibility** has been greatly improved: compatible brands in ftyp box
  has been checked, and now it can support various compatible MP4/MOV files.

## nom-exif v1.2.3

[2861cbc..8c00f1b](https://github.com/mindeng/nom-exif/compare/2861cbc..8c00f1b)

### Fixed

- **All** clippy warnings has been fixed!

### Changed

- **Deprecated** some less commonly used APIs and introduced several new ones,
  mainly to satisfy clippy requirements, e.g.:

  - `GPSInfo.to_iso6709` -> `format_iso6709`
  - `URational.to_float` -> `as_float`
  
  See commit [8c5dc26](https://github.com/mindeng/nom-exif/commit/8c5dc26).

## nom-exif v1.2.2

[9b7fdf7..2861cbc](https://github.com/mindeng/nom-exif/compare/9b7fdf7..2861cbc)

### Added

- **Fuzz testing**: Added *afl-fuzz* for fuzz testing.

### Changed

### Fixed

- **Robustness improved**: Fixed all crash issues discovered during fuzz
  testing.
- **Clippy warnings**: Checked with the latest clippy and fixed almost all of
  the warnings.

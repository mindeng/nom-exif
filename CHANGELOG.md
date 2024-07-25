# Changelog

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

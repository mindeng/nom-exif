# Changelog

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

# rexiftool

[![crates.io](https://img.shields.io/crates/v/rexiftool.svg)](https://crates.io/crates/rexiftool)

Pretty-print EXIF and video / audio track metadata for image, video,
and audio files. Companion CLI to the
[`nom-exif`](https://crates.io/crates/nom-exif) library.

Pure Rust — no FFmpeg, no libexif, no system deps.

## Install

```sh
cargo install rexiftool
```

Pre-built binaries (macOS Intel / Apple Silicon, Linux x86_64,
Windows x86_64) are attached to each `rexiftool-v*` release on
[GitHub Releases](https://github.com/mindeng/nom-exif/releases).

## Usage

```sh
# Single file (image, video, or audio):
rexiftool photo.heic

# JSON output:
rexiftool photo.heic -j

# Batch (non-recursive directory):
rexiftool ./photos/

# Tracing logs to ./debug.log:
rexiftool --debug photo.heic
```

Flags:

- `-j, --json` — JSON output instead of `key : value`. JSON is never
  truncated and always includes thumbnail (IFD1) entries.
- `--no-track` — skip embedded media tracks (e.g. Pixel Motion Photo
  MP4 trailers).
- `--no-format` — skip format-specific metadata (e.g. PNG `tEXt`
  chunks).
- `--with-thumbnail` — include thumbnail (IFD1) entries; hidden by
  default because they mostly duplicate the main image's tags.
- `--full` — print full values without per-line / per-value
  truncation. By default each line is capped at 200 chars and each
  value at 10 lines.
- `--debug` — write tracing logs to `./debug.log`.

## Supported formats

JPEG, HEIC / HEIF, PNG, AVIF, TIFF, Canon CR3, Fujifilm RAF, Phase One
IIQ, MOV / MP4 / 3GP, MKV / WEBM / MKA, and more. See the
[main README](https://github.com/mindeng/nom-exif#supported-file-types)
for the full list.

## License

MIT

# Nom-Exif

[![crates.io](https://img.shields.io/crates/v/nom-exif.svg)](https://crates.io/crates/nom-exif)
[![Documentation](https://docs.rs/nom-exif/badge.svg)](https://docs.rs/nom-exif)
[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mindeng/nom-exif/actions/workflows/rust.yml/badge.svg)](https://github.com/mindeng/nom-exif/actions)

Exif/metadata parsing library written in pure Rust with [nom](https://github.com/rust-bakery/nom).

## Supported File Types

-   Images
    -   JPEG
    -   HEIF/HEIC
-   Videos
    -   MOV
    -   MP4

## Features

-   **Zero-copy when appropriate**: Use borrowing and slicing instead of copying
    whenever possible.
-   **Minimize I/O operations**: When metadata is stored at the end/middle of a
    large file (such as a MOV/HEIC file does), `Seek` rather than `Read` to
    quickly locate the location of the metadata.
-   **Pay as you go**: When extracting Exif data, only the information
    corresponding to the specified Exif tags are parsed to reduce the overhead
    when processing a large number of files.
-   **Robustness and stability**: Through long-term [Fuzz
    testing](https://github.com/rust-fuzz/afl.rs), and tons of crash issues
    discovered during testing have been fixed. Thanks to
    [@sigaloid](https://github.com/sigaloid) for [raising this
    question](https://github.com/mindeng/nom-exif/pull/5).


## Usage

- Images
    - [`parse_heif_exif`](https://docs.rs/nom-exif/latest/nom_exif/fn.parse_heif_exif.html)
    - [`parse_jpeg_exif`](https://docs.rs/nom-exif/latest/nom_exif/fn.parse_jpeg_exif.html)
- Videos
    - [`parse_metadata`](https://docs.rs/nom-exif/latest/nom_exif/fn.parse_metadata.html)
- [examples](examples/)

## CLI Tool `rexiftool`

### Normal output

`cargo run --example rexiftool testdata/meta.mov`:

``` text
com.apple.quicktime.make                => Apple
com.apple.quicktime.model               => iPhone X
com.apple.quicktime.software            => 12.1.2
com.apple.quicktime.location.ISO6709    => +27.1281+100.2508+000.000/
com.apple.quicktime.creationdate        => 2019-02-12T15:27:12+08:00
duration                                => 500
width                                   => 720
height                                  => 1280
```

### Json dump

`cargo run --features json_dump --example rexiftool -- -j testdata/meta.mov`:

``` text
{
  "height": "1280",
  "duration": "500",
  "width": "720",
  "com.apple.quicktime.creationdate": "2019-02-12T15:27:12+08:00",
  "com.apple.quicktime.make": "Apple",
  "com.apple.quicktime.model": "iPhone X",
  "com.apple.quicktime.software": "12.1.2",
  "com.apple.quicktime.location.ISO6709": "+27.1281+100.2508+000.000/"
}
```

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

-   **Zero-copy when appropriate:** Use borrowing and slicing instead of copying
    whenever possible.
-   **Minimize I/O operations:** When metadata is stored at the end of a larger file
    (such as a MOV file), `Seek` rather than `Read` to quickly locate the location of
    the metadata.
-   **Pay as you go:** When extracting Exif data, only the information corresponding
    to the specified Exif tags are parsed to reduce the overhead when processing a
    large number of files.

## Usage

- [`parse_heif_exif`](https://docs.rs/nom-exif/latest/nom_exif/fn.parse_heif_exif.html)
- [`parse_jpeg_exif`](https://docs.rs/nom-exif/latest/nom_exif/fn.parse_jpeg_exif.html)
- [`parse_metadata`](https://docs.rs/nom-exif/latest/nom_exif/fn.parse_metadata.html)
- [examples](examples/)

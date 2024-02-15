<!-- # Table of Contents -->

<!-- 1.  [Nom-Exif](#orgcd5539d) -->
<!--     1.  [Supported File Types](#org85ad222) -->
<!--     2.  [Features](#org6d43624) -->
<!--     3.  [Usage](#org55e3f0a) -->
<!--         1.  [Parse Exif from Images](#orgd388296) -->
<!--         2.  [Parse metadata from Videos](#orgf7ffad0) -->


<a id="orgcd5539d"></a>

# Nom-Exif

![nom-exif workflow](https://github.com/mindeng/nom-exif/actions/workflows/rust.yml/badge.svg)

Exif/metadata parsing library written in pure Rust with [nom](https://github.com/rust-bakery/nom).


<a id="org85ad222"></a>

## Supported File Types

-   Images
    -   JPEG
    -   HEIF/HEIC
-   Videos
    -   MOV
    -   MP4


<a id="org6d43624"></a>

## Features

-   **Zero-copy when appropriate:** Use borrowing and slicing instead of copying
    whenever possible.
-   **Minimize I/O operations:** When metadata is stored at the end of a larger file
    (such as a MOV file), `Seek` rather than `Read` to quickly locate the location of
    the metadata.
-   **Pay as you go:** When extracting Exif data, only the information corresponding
    to the specified Exif tags are parsed to reduce the overhead when processing a
    large number of files.


<a id="org55e3f0a"></a>

## Usage


<a id="orgd388296"></a>

### Parse Exif from Images

``` rust
use nom_exif::*;
use nom_exif::ExifTag::*;

use std::fs::File;
use std::path::Path;
use std::collections::HashMap;

let f = File::open(Path::new("./testdata/exif.jpg")).unwrap();
let exif = parse_jpeg_exif(f).unwrap().unwrap();

assert_eq!(
    exif.get_value(&ImageWidth).unwrap(),
    Some(IfdEntryValue::U32(3072)));

assert_eq!(
    exif.get_values(&[CreateDate, ModifyDate, DateTimeOriginal]),
    [
        (&CreateDate, "2023:07:09 20:36:33"),
        (&ModifyDate, "2023:07:09 20:36:33"),
        (&DateTimeOriginal, "2023:07:09 20:36:33"),
    ]
    .into_iter()
    .map(|x| (x.0, x.1.into()))
    .collect::<HashMap<_, _>>()
);
```


<a id="orgf7ffad0"></a>

### Parse metadata from Videos

``` rust
use nom_exif::*;

use std::fs::File;
use std::path::Path;

let f = File::open(Path::new("./testdata/meta.mov")).unwrap();
let entries = parse_mov_metadata(reader).unwrap();

assert_eq!(
    entries
        .iter()
        .map(|x| format!("{x:?}"))
        .collect::<Vec<_>>()
        .join("\n"),
    r#"("com.apple.quicktime.make", Text("Apple"))
("com.apple.quicktime.model", Text("iPhone X"))
("com.apple.quicktime.software", Text("12.1.2"))
("com.apple.quicktime.location.ISO6709", Text("+27.1281+100.2508+000.000/"))
("com.apple.quicktime.creationdate", Text("2019-02-12T15:27:12+08:00"))"#
);
```

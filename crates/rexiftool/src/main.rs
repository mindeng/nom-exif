use std::{
    error::Error,
    fs::{self, File},
    io::{self},
    path::Path,
    process::ExitCode,
};

use clap::Parser;
use nom_exif::{ExifIter, ImageFormatMetadata, MediaKind, MediaParser, MediaSource, TrackInfo};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Registry};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    file: String,

    #[arg(short, long)]
    json: bool,

    /// Skip extracting embedded media tracks (e.g. Pixel Motion Photo MP4
    /// trailers). By default, when an image carries an embedded track,
    /// its metadata is appended after the EXIF entries.
    #[arg(long)]
    no_track: bool,

    /// Skip printing format-specific metadata (e.g. PNG tEXt chunks).
    /// By default, when an image carries format-specific metadata,
    /// it is appended under a "-- Format Metadata --" section.
    #[arg(long)]
    no_format: bool,

    /// Include thumbnail (IFD1) entries. By default they are hidden,
    /// because they mostly duplicate the main image's tags
    /// (XResolution, ExifImageWidth, …).
    #[arg(long)]
    with_thumbnail: bool,

    /// Print full values without per-line / per-value truncation.
    /// By default rexiftool caps each line at 200 chars and each
    /// value at 10 lines so embedded hex blobs (e.g. PNG tEXt chunks
    /// carrying raw EXIF) don't swamp the terminal. JSON output is
    /// always unbounded.
    #[arg(long)]
    full: bool,

    #[arg(long)]
    debug: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    tracing_run(&cli)
}

#[tracing::instrument]
fn tracing_run(cli: &Cli) -> ExitCode {
    if cli.debug {
        init_tracing().expect("init tracing failed");
    }

    if let Err(err) = run(cli) {
        tracing::error!(?err);
        eprintln!("{err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn run(cli: &Cli) -> Result<(), Box<dyn Error>> {
    let mut parser = MediaParser::new();

    let path = Path::new(&cli.file);
    if path.is_file() {
        let _ = parse_file(&mut parser, path, cli);
    } else if path.is_dir() {
        parse_dir(path, parser, cli)?;
    }

    Ok(())
}

fn parse_dir(path: &Path, mut parser: MediaParser, cli: &Cli) -> Result<(), Box<dyn Error>> {
    let mut first = true;
    for entry in fs::read_dir(path)? {
        if first {
            first = false;
        } else {
            println!();
        }
        match entry {
            Ok(entry) => {
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if !ft.is_file() {
                    continue;
                }
                let p = entry.path();
                println!("File: {}", p.display());
                println!("{}", "-".repeat(SECTION_WIDTH));
                let _ = parse_file(&mut parser, p, cli);
            }
            Err(e) => {
                eprintln!("Read dir entry failed: {e}");
                continue;
            }
        }
    }
    Ok(())
}

fn parse_file<P: AsRef<Path>>(
    parser: &mut MediaParser,
    path: P,
    cli: &Cli,
) -> Result<(), nom_exif::Error> {
    let path = path.as_ref();
    let ms = MediaSource::open(path).inspect_err(handle_parsing_error)?;
    let (values, embedded, format_pairs) = match ms.kind() {
        MediaKind::Image => {
            // Single parse_image_metadata call yields both EXIF and
            // format-specific metadata; saves a redundant file-reopen per
            // PNG. parse_image_metadata returns ExifNotFound when neither
            // EXIF nor format metadata is present (e.g. JPEG with no
            // EXIF) — treat as empty so non-image-bearing files still
            // exit cleanly.
            let img_result = parser.parse_image_metadata(ms);
            let (exif_values, has_embedded, fmt_pairs) = match img_result {
                Ok(img) => {
                    let (has, exif_pairs) = match img.exif {
                        Some(iter) => (
                            iter.has_embedded_track(),
                            exif_iter_to_pairs(iter, cli.with_thumbnail),
                        ),
                        None => (false, vec![]),
                    };
                    let fmt_pairs: Option<Vec<(String, String)>> = if cli.no_format {
                        None
                    } else {
                        match img.format {
                            Some(ImageFormatMetadata::Png(text_chunks))
                                if !text_chunks.is_empty() =>
                            {
                                Some(
                                    text_chunks
                                        .iter()
                                        .map(|(k, v)| (k.to_owned(), v.to_owned()))
                                        .collect(),
                                )
                            }
                            _ => None,
                        }
                    };
                    (exif_pairs, has, fmt_pairs)
                }
                Err(nom_exif::Error::ExifNotFound) => (vec![], false, None),
                Err(e) => {
                    handle_parsing_error(&e);
                    return Err(e);
                }
            };

            // When the image carries an embedded media track (e.g. a Pixel
            // Motion Photo MP4 trailer), surface its metadata too — unless
            // the user opted out with --no-track. parse_image_metadata
            // consumed the MediaSource, so re-open the path.
            let track_values = if has_embedded && !cli.no_track {
                match MediaSource::open(path).and_then(|ms| parser.parse_track(ms)) {
                    Ok(info) => Some(track_info_to_pairs(&info)),
                    Err(e) => {
                        eprintln!(
                            "Warning: image flags an embedded track but parse_track failed: {e}"
                        );
                        None
                    }
                }
            } else {
                None
            };

            (exif_values, track_values, fmt_pairs)
        }
        MediaKind::Track => {
            let info: TrackInfo = parser.parse_track(ms)?;
            (track_info_to_pairs(&info), None, None)
        }
    };
    if cli.json {
        emit_json(&values, embedded.as_deref(), format_pairs.as_deref());
    } else {
        let has_extra = embedded.is_some() || format_pairs.is_some();
        let key_width = compute_key_width(
            values.iter().map(|(k, _)| k.as_str()).chain(
                embedded
                    .iter()
                    .flat_map(|t| t.iter().map(|(k, _)| k.as_str())),
            ),
        );
        let fmt_key_width = format_pairs
            .as_deref()
            .map(|p| compute_key_width(p.iter().map(|(k, _)| k.as_str())))
            .unwrap_or(MIN_KEY_WIDTH);

        let mut printed_section = false;
        if has_extra && !values.is_empty() {
            println!("{}", section_header("EXIF"));
            printed_section = true;
        }
        for (k, v) in &values {
            print_pair(k, &v.to_string(), key_width, cli.full);
        }
        if let Some(track) = &embedded {
            if printed_section || !values.is_empty() {
                println!();
            }
            println!("{}", section_header("Embedded Track"));
            printed_section = true;
            for (k, v) in track {
                print_pair(k, &v.to_string(), key_width, cli.full);
            }
        }
        if let Some(fmt) = &format_pairs {
            if printed_section || !values.is_empty() {
                println!();
            }
            println!("{}", section_header("Format Metadata"));
            for (k, v) in fmt {
                print_pair(k, v, fmt_key_width, cli.full);
            }
        }
    };
    Ok(())
}

const KV_SEP: &str = ": ";
const MAX_LINE_CHARS: usize = 200;
const MAX_VALUE_LINES: usize = 10;

fn print_pair(key: &str, value: &str, key_width: usize, full: bool) {
    if value.is_empty() {
        println!("{:<width$}{KV_SEP}(empty)", key, width = key_width);
        return;
    }
    let indent = " ".repeat(key_width + KV_SEP.chars().count());
    let total_lines = value.split('\n').count();
    let line_budget = if full { usize::MAX } else { MAX_VALUE_LINES };

    for (i, line) in value.split('\n').enumerate() {
        if i >= line_budget {
            let remaining = total_lines - i;
            println!("{indent}… (+{remaining} more line{})", plural(remaining));
            break;
        }
        let rendered = if full {
            line.to_owned()
        } else {
            truncate_line(line, MAX_LINE_CHARS)
        };
        if i == 0 {
            println!("{:<width$}{KV_SEP}{rendered}", key, width = key_width);
        } else {
            println!("{indent}{rendered}");
        }
    }
}

fn truncate_line(line: &str, max_chars: usize) -> String {
    let count = line.chars().count();
    if count <= max_chars {
        return line.to_owned();
    }
    let head: String = line.chars().take(max_chars).collect();
    let extra = count - max_chars;
    format!("{head}… (+{extra} char{})", plural(extra))
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

const SECTION_WIDTH: usize = 48;
const MIN_KEY_WIDTH: usize = 32;
const MAX_KEY_WIDTH: usize = 48;

fn section_header(title: &str) -> String {
    let prefix = format!("-- {title} ");
    let pad = SECTION_WIDTH.saturating_sub(prefix.chars().count());
    format!("{prefix}{}", "-".repeat(pad))
}

fn compute_key_width<'a, I: Iterator<Item = &'a str>>(keys: I) -> usize {
    keys.map(|k| k.chars().count())
        .max()
        .map(|m| m.saturating_add(1).clamp(MIN_KEY_WIDTH, MAX_KEY_WIDTH))
        .unwrap_or(MIN_KEY_WIDTH)
}

fn exif_iter_to_pairs(iter: ExifIter, with_thumbnail: bool) -> Vec<(String, nom_exif::EntryValue)> {
    iter.into_iter()
        .filter_map(|x| {
            if !with_thumbnail && x.ifd() == nom_exif::IfdIndex::THUMBNAIL {
                return None;
            }
            let tag = x.tag();
            match x.into_result() {
                Ok(v) => Some((
                    match tag {
                        nom_exif::TagOrCode::Tag(t) => t.to_string(),
                        nom_exif::TagOrCode::Unknown(c) => format!("Unknown(0x{c:04x})"),
                    },
                    v,
                )),
                Err(e) => {
                    tracing::warn!(?e);
                    None
                }
            }
        })
        .collect()
}

fn track_info_to_pairs(info: &TrackInfo) -> Vec<(String, nom_exif::EntryValue)> {
    info.iter()
        .map(|(tag, val)| (tag.to_string(), val.clone()))
        .collect()
}

fn emit_json(
    values: &[(String, nom_exif::EntryValue)],
    embedded: Option<&[(String, nom_exif::EntryValue)]>,
    format_pairs: Option<&[(String, String)]>,
) {
    use serde_json::{Map, Value};
    let mut root = Map::with_capacity(values.len() + 1);
    for (k, v) in values {
        if let Ok(json) = serde_json::to_value(v) {
            root.insert(k.clone(), json);
        }
    }
    if let Some(track) = embedded {
        let mut nested = Map::with_capacity(track.len());
        for (k, v) in track {
            if let Ok(json) = serde_json::to_value(v) {
                nested.insert(k.clone(), json);
            }
        }
        root.insert("_embedded_track".into(), Value::Object(nested));
    }
    if let Some(fmt) = format_pairs {
        let mut nested = Map::with_capacity(fmt.len());
        for (k, v) in fmt {
            nested.insert(k.clone(), Value::String(v.clone()));
        }
        root.insert("_format".into(), Value::Object(nested));
    }
    match serde_json::to_string_pretty(&Value::Object(root)) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("Error: {e}"),
    }
}

fn handle_parsing_error(e: &nom_exif::Error) {
    match e {
        nom_exif::Error::UnsupportedFormat => {
            eprintln!("Unrecognized file format, consider filing a bug @ https://github.com/mindeng/nom-exif.");
        }
        _ => {
            eprintln!("Error: {e}");
        }
    }
}

fn init_tracing() -> io::Result<()> {
    let stdout_log = tracing_subscriber::fmt::layer().pretty();
    let subscriber = Registry::default().with(stdout_log);

    let file = File::create("debug.log")?;
    let debug_log = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file);
    let subscriber = subscriber.with(debug_log);

    subscriber.init();

    Ok(())
}

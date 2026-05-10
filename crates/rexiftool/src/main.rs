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
                        Some(iter) => (iter.has_embedded_track(), exif_iter_to_pairs(iter)),
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

        if has_extra && !values.is_empty() {
            println!("{}", section_header("EXIF"));
        }
        values.iter().for_each(|x| {
            println!("{:<width$}=> {}", x.0, x.1, width = key_width);
        });
        if let Some(track) = &embedded {
            println!("{}", section_header("Embedded Track"));
            track.iter().for_each(|x| {
                println!("{:<width$}=> {}", x.0, x.1, width = key_width);
            });
        }
        if let Some(fmt) = &format_pairs {
            println!("{}", section_header("Format Metadata"));
            fmt.iter().for_each(|(k, v)| {
                println!("{:<width$}=> {}", k, v, width = fmt_key_width);
            });
        }
    };
    Ok(())
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

fn exif_iter_to_pairs(iter: ExifIter) -> Vec<(String, nom_exif::EntryValue)> {
    iter.into_iter()
        .filter_map(|x| {
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

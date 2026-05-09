use std::{
    error::Error,
    fs::{self, File},
    io::{self},
    path::Path,
    process::ExitCode,
};

use clap::Parser;
use nom_exif::{ExifIter, MediaKind, MediaParser, MediaSource, TrackInfo};
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

    #[arg(long)]
    debug: bool,
}

#[cfg(feature = "serde")]
const FEATURE_SERDE_ON: bool = true;
#[cfg(not(feature = "serde"))]
const FEATURE_SERDE_ON: bool = false;

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
    if cli.json && !FEATURE_SERDE_ON {
        let msg = "-j/--json option requires the feature `serde`.";
        eprintln!("{msg}");
        return Err(msg.into());
    }

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
                println!("File: {:?}", entry.path().as_os_str());
                println!("------------------------------------------------");
                let _ = parse_file(&mut parser, entry.path(), cli);
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
    let (values, embedded) = match ms.kind() {
        MediaKind::Image => {
            let iter: ExifIter = parser.parse_exif(ms).inspect_err(handle_parsing_error)?;
            let has_embedded = iter.has_embedded_track();
            let exif_values = exif_iter_to_pairs(iter);

            // When the image carries an embedded media track (e.g. a Pixel
            // Motion Photo MP4 trailer), surface its metadata too — unless
            // the user opted out with --no-track. parse_exif consumed the
            // MediaSource, so re-open the path.
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
            (exif_values, track_values)
        }
        MediaKind::Track => {
            let info: TrackInfo = parser.parse_track(ms)?;
            (track_info_to_pairs(&info), None)
        }
    };
    if cli.json {
        #[cfg(feature = "serde")]
        emit_json(&values, embedded.as_deref());
    } else {
        values.iter().for_each(|x| {
            println!("{:<32}=> {}", x.0, x.1);
        });
        if let Some(track) = &embedded {
            println!("-- Embedded Track ------------------------------");
            track.iter().for_each(|x| {
                println!("{:<32}=> {}", x.0, x.1);
            });
        }
    };
    Ok(())
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

#[cfg(feature = "serde")]
fn emit_json(
    values: &[(String, nom_exif::EntryValue)],
    embedded: Option<&[(String, nom_exif::EntryValue)]>,
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

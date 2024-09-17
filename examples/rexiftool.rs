use std::{
    error::Error,
    fs::{self, File},
    io::{self},
    path::Path,
    process::ExitCode,
};

use clap::Parser;
use nom_exif::{ExifIter, MediaParser, MediaSource, TrackInfo};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Registry};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    file: String,

    #[arg(short, long)]
    json: bool,

    #[arg(long)]
    debug: bool,
}

#[cfg(feature = "json_dump")]
const FEATURE_JSON_DUMP_ON: bool = true;
#[cfg(not(feature = "json_dump"))]
const FEATURE_JSON_DUMP_ON: bool = false;

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
    if cli.json && !FEATURE_JSON_DUMP_ON {
        let msg = "-j/--json option requires the feature `json_dump`.";
        eprintln!("{msg}");
        return Err(msg.into());
    }

    let mut parser = MediaParser::new();

    let path = Path::new(&cli.file);
    if path.is_file() {
        let _ = parse_file(&mut parser, path, cli.json);
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
                let _ = parse_file(&mut parser, entry.path(), cli.json);
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
    json: bool,
) -> Result<(), nom_exif::Error> {
    let ms = MediaSource::file_path(path).inspect_err(handle_parsing_error)?;
    let values = if ms.has_exif() {
        let iter: ExifIter = parser.parse(ms).inspect_err(handle_parsing_error)?;
        iter.into_iter()
            .filter_map(|mut x| {
                let res = x.take_result();
                match res {
                    Ok(v) => Some((
                        x.tag()
                            .map(|x| x.to_string())
                            .unwrap_or_else(|| format!("Unknown(0x{:04x})", x.tag_code())),
                        v,
                    )),
                    Err(e) => {
                        tracing::warn!(?e);
                        None
                    }
                }
            })
            .collect::<Vec<_>>()
    } else {
        let info: TrackInfo = parser.parse(ms)?;
        info.into_iter()
            .map(|x| (x.0.to_string(), x.1))
            .collect::<Vec<_>>()
    };
    if json {
        #[cfg(feature = "json_dump")]
        use std::collections::HashMap;

        #[cfg(feature = "json_dump")]
        match serde_json::to_string_pretty(
            &values
                .into_iter()
                .map(|x| (x.0.to_string(), x.1))
                .collect::<HashMap<_, _>>(),
        ) {
            Ok(s) => {
                println!("{}", s);
            }
            Err(e) => eprintln!("Error: {e}"),
        }
    } else {
        values.iter().for_each(|x| {
            println!("{:<32}=> {}", x.0, x.1);
        });
    };
    Ok(())
}

fn handle_parsing_error(e: &nom_exif::Error) {
    match e {
        nom_exif::Error::UnrecognizedFileFormat => {
            eprintln!("Unrecognized file format, consider filing a bug @ https://github.com/mindeng/nom-exif.");
        }
        nom_exif::Error::ParseFailed(_) | nom_exif::Error::IOError(_) => {
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

use std::{
    error::Error,
    fs::File,
    io::{self, Seek},
    process::ExitCode,
};

use clap::Parser;
use nom_exif::{parse_exif, FileFormat};
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
    let mut reader = File::open(&cli.file)?;
    let ff = FileFormat::try_from_read(&mut reader)
        .map_err(|e| format!("unsupported file format: {e}"))?;
    reader.seek(io::SeekFrom::Start(0))?;

    if cli.json && !FEATURE_JSON_DUMP_ON {
        let msg = "-j/--json option requires the feature `json_dump`.";
        eprintln!("{msg}");
        return Err(msg.into());
    }

    let values = match ff {
        FileFormat::Jpeg | FileFormat::Heif => {
            let iter = parse_exif(&mut reader, Some(ff))?;
            let Some(iter) = iter else {
                println!("Exif data not found in {}.", &cli.file);
                return Ok(());
            };
            iter.filter_map(|x| {
                let v = x.take_value()?;
                Some((
                    x.tag()
                        .map(|x| x.to_string())
                        .unwrap_or_else(|| format!("0x{:04x}", x.tag_code())),
                    v,
                ))
            })
            .collect::<Vec<_>>()
        }
        FileFormat::QuickTime | FileFormat::MP4 => {
            let meta = nom_exif::parse_metadata(&mut reader)?;
            meta.into_iter()
                .map(|x| (x.0.to_string(), x.1))
                .collect::<Vec<_>>()
        }
        FileFormat::WEBM => {
            return Err(format!("unsupported file format: {ff}").into());
        }
    };

    if cli.json {
        #[cfg(feature = "json_dump")]
        use std::collections::HashMap;

        #[cfg(feature = "json_dump")]
        println!(
            "{}",
            serde_json::to_string_pretty(
                &values
                    .into_iter()
                    .map(|x| (x.0.to_string(), x.1))
                    .collect::<HashMap<_, _>>()
            )?
        );
    } else {
        values.iter().for_each(|x| {
            println!("{:<40}=> {}", x.0, x.1);
        });
    }

    Ok(())
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

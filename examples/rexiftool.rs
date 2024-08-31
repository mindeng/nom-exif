use std::{error::Error, ffi::OsStr, fs::File, path::Path};

use clap::Parser;
use nom_exif::parse_exif;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    file: String,

    #[arg(short, long)]
    json: bool,
}

#[cfg(feature = "json_dump")]
const FEATURE_JSON_DUMP_ON: bool = true;
#[cfg(not(feature = "json_dump"))]
const FEATURE_JSON_DUMP_ON: bool = false;

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let Some(extension) = Path::new(&cli.file).extension().and_then(OsStr::to_str) else {
        return Ok(());
    };

    if cli.json && !FEATURE_JSON_DUMP_ON {
        let msg = "-j/--json option requires the feature `json_dump`.";
        eprintln!("{msg}");
        return Err(msg.into());
    }

    let extension = extension.to_lowercase();
    let mut reader = File::open(&cli.file)?;
    let values = match extension.as_ref() {
        "jpg" | "jpeg" | "heic" | "heif" => {
            let iter = parse_exif(&mut reader, None)?;
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
        "mov" | "mp4" => {
            let meta = nom_exif::parse_metadata(&mut reader)?;
            meta.into_iter()
                .map(|x| (x.0.to_string(), x.1))
                .collect::<Vec<_>>()
        }
        other => {
            eprintln!("Unsupported filetype: {other}");
            return Err("Unsupported filetype".into());
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

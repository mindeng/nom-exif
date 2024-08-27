use std::{error::Error, ffi::OsStr, fs::File, path::Path};

use clap::Parser;
use nom_exif::ExifTag::{self, *};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    file: String,

    #[arg(short, long)]
    json: bool,
}

const TAGS: &[ExifTag] = &[
    Make,
    Model,
    Orientation,
    ImageWidth,
    ImageHeight,
    ISOSpeedRatings,
    ShutterSpeedValue,
    ExposureTime,
    FNumber,
    ExifImageWidth,
    ExifImageHeight,
    DateTimeOriginal,
    CreateDate,
    ModifyDate,
    OffsetTimeOriginal,
    OffsetTime,
    GPSLatitudeRef,
    GPSLatitude,
    GPSLongitudeRef,
    GPSLongitude,
    GPSAltitudeRef,
    GPSAltitude,
    GPSVersionID,
    // sub ifd
    ExifOffset,
    GPSInfo,
    ImageDescription,
    XResolution,
    YResolution,
    ResolutionUnit,
    Software,
    HostComputer,
    WhitePoint,
    PrimaryChromaticities,
    YCbCrCoefficients,
    ReferenceBlackWhite,
    Copyright,
    ExposureProgram,
    SpectralSensitivity,
    OECF,
    SensitivityType,
    ExifVersion,
    ApertureValue,
    BrightnessValue,
    ExposureBiasValue,
    MaxApertureValue,
    SubjectDistance,
    MeteringMode,
    LightSource,
    Flash,
    FocalLength,
    SubjectArea,
    MakerNote,
    // UserComment,
    FlashPixVersion,
    ColorSpace,
    RelatedSoundFile,
    FlashEnergy,
    FocalPlaneXResolution,
    FocalPlaneYResolution,
    FocalPlaneResolutionUnit,
    SubjectLocation,
    ExposureIndex,
    SensingMethod,
    FileSource,
    SceneType,
    CFAPattern,
    CustomRendered,
    ExposureMode,
    WhiteBalanceMode,
    DigitalZoomRatio,
    FocalLengthIn35mmFilm,
    SceneCaptureType,
    GainControl,
    Contrast,
    Saturation,
    Sharpness,
    DeviceSettingDescription,
    SubjectDistanceRange,
    ImageUniqueID,
    LensSpecification,
    LensMake,
    LensModel,
    Gamma,
    GPSTimeStamp,
    GPSSatellites,
    GPSStatus,
    GPSMeasureMode,
    GPSDOP,
    GPSSpeedRef,
    GPSSpeed,
    GPSTrackRef,
    GPSTrack,
    GPSImgDirectionRef,
    GPSImgDirection,
    GPSMapDatum,
    GPSDestLatitudeRef,
    GPSDestLatitude,
    GPSDestLongitudeRef,
    GPSDestLongitude,
    GPSDestBearingRef,
    GPSDestBearing,
    GPSDestDistanceRef,
    GPSDestDistance,
    GPSProcessingMethod,
    GPSAreaInformation,
    GPSDateStamp,
    GPSDifferential,
];

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
        "jpg" | "jpeg" => {
            let exif = nom_exif::parse_jpeg_exif(&mut reader)?;
            let Some(exif) = exif else {
                return Ok(());
            };
            exif.get_values(TAGS)
                .into_iter()
                .map(|x| (x.0.to_string(), x.1))
                .collect::<Vec<_>>()
        }
        "heic" | "heif" => {
            let exif = nom_exif::parse_heif_exif(&mut reader)?;
            let Some(exif) = exif else {
                return Ok(());
            };
            exif.get_values(TAGS)
                .into_iter()
                .map(|x| (x.0.to_string(), x.1))
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

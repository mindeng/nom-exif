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
    Unknown,
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

#[cfg(feature = "serialize")]
const FEATURE_SERIALIZE_ON: bool = true;
#[cfg(not(feature = "serialize"))]
const FEATURE_SERIALIZE_ON: bool = false;

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let Some(extension) = Path::new(&cli.file).extension().and_then(OsStr::to_str) else {
        return Ok(());
    };

    let extension = extension.to_lowercase();
    match extension.as_ref() {
        "jpg" | "jpeg" | "heic" | "heif" => {
            if cli.json && !FEATURE_SERIALIZE_ON {
                let msg = "-j/--json option requires the feature `serialize`.";
                eprintln!("{msg}");
                return Err(msg.into());
            }

            let exif = nom_exif::parse_exif(&cli.file)?;
            if let Some(exif) = exif {
                let values = exif.get_values(TAGS);

                if cli.json {
                    #[cfg(feature = "serialize")]
                    println!("{}", serde_json::to_string_pretty(&values)?);
                } else {
                    let mut entries = values
                        .into_iter()
                        .map(|x| format!("{:<32}-> {}", x.0.to_string(), x.1))
                        .collect::<Vec<_>>();
                    entries.sort();
                    entries.iter().for_each(|x| {
                        println!("{x}");
                    });
                }
            }
        }
        "mov" | "mp4" => {
            let mut reader = File::open(&cli.file)?;
            let mut meta = nom_exif::parse_metadata(&mut reader)?;
            meta.sort_by(|(ref x, _), (ref y, _)| x.cmp(y));
            meta.iter().for_each(|x| {
                println!("{:<50}-> {}", x.0, x.1);
            });
        }
        other => {
            println!("Unsupported filetype: {other}")
        }
    }

    Ok(())
}

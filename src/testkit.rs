use std::{fs::File, io::Read, path::Path};

use crate::exif::Exif;
use crate::exif::ExifTag::*;

pub fn read_sample(path: &str) -> Result<Vec<u8>, std::io::Error> {
    let mut f = open_sample(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn open_sample(path: &str) -> Result<File, std::io::Error> {
    let p = Path::new(path);
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new("./testdata").join(p)
    };
    File::open(p)
}

#[allow(unused)]
pub fn open_sample_w(path: &str) -> Result<File, std::io::Error> {
    let p = Path::new(path);
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new("./testdata").join(p)
    };
    File::create(p)
}

pub fn sorted_exif_entries(exif: &Exif) -> Vec<String> {
    let tags = [
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
        CameraSerialNumber,
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
    let mut entries = tags
        .iter()
        .filter_map(|tag| exif.get(*tag).map(|v| format!("{} » {}", tag, v)))
        .collect::<Vec<_>>();
    entries.sort();

    entries
}

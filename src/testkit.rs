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
    if p.is_absolute() {
        File::open(p)
    } else {
        File::open(Path::new("./testdata").join(p))
    }
}

pub fn sorted_exif_entries(exif: &Exif) -> Vec<String> {
    let mut entries = exif
        .get_values(&[
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
        ])
        .into_iter()
        .map(|x| format!("{} » {}", x.0, x.1))
        .collect::<Vec<_>>();
    entries.sort();

    entries
}

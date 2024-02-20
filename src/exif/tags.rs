//! Define exif tags and related enums, see
//! https://exiftool.org/TagNames/EXIF.html

use std::fmt::Display;

#[cfg(feature = "json_dump")]
use serde::{Deserialize, Serialize};

#[allow(unused)]
#[cfg_attr(feature = "json_dump", derive(Serialize, Deserialize))]
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub enum ExifTag {
    Unknown = 0x0000_ffff,

    Make = 0x0000_010f,
    Model = 0x0000_0110,
    Orientation = 0x0000_0112,

    ImageWidth = 0x0100,
    ImageHeight = 0x0101,

    ISOSpeedRatings = 0x0000_8827,
    ShutterSpeedValue = 0x0000_9201,
    ExposureTime = 0x0000_829a,
    FNumber = 0x0000_829d,

    ExifImageWidth = 0xa002,
    ExifImageHeight = 0xa003,

    DateTimeOriginal = 0x0000_9003,
    CreateDate = 0x0000_9004,
    ModifyDate = 0x0000_0132,
    OffsetTimeOriginal = 0x0000_9011,
    OffsetTime = 0x0000_9010,

    GPSLatitudeRef = 0x00001,
    GPSLatitude = 0x00002,
    GPSLongitudeRef = 0x00003,
    GPSLongitude = 0x00004,
    GPSAltitudeRef = 0x00005,
    GPSAltitude = 0x00006,
    GPSVersionID = 0x00000,

    // sub ifd
    ExifOffset = 0x0000_8769,
    GPSInfo = 0x0000_8825,

    ImageDescription = 0x0000_010e,
    XResolution = 0x0000_011a,
    YResolution = 0x0000_011b,
    ResolutionUnit = 0x0000_0128,
    Software = 0x0000_0131,
    HostComputer = 0x0000_013c,
    WhitePoint = 0x0000_013e,
    PrimaryChromaticities = 0x0000_013f,
    YCbCrCoefficients = 0x0000_0211,
    ReferenceBlackWhite = 0x0000_0214,
    Copyright = 0x0000_8298,

    ExposureProgram = 0x0000_8822,
    SpectralSensitivity = 0x0000_8824,
    OECF = 0x0000_8828,
    SensitivityType = 0x0000_8830,
    ExifVersion = 0x0000_9000,
    ApertureValue = 0x0000_9202,
    BrightnessValue = 0x0000_9203,
    ExposureBiasValue = 0x0000_9204,
    MaxApertureValue = 0x0000_9205,
    SubjectDistance = 0x0000_9206,
    MeteringMode = 0x0000_9207,
    LightSource = 0x0000_9208,
    Flash = 0x0000_9209,
    FocalLength = 0x0000_920a,
    SubjectArea = 0x0000_9214,
    MakerNote = 0x0000_927c,
    UserComment = 0x0000_9286,
    FlashPixVersion = 0x0000_a000,
    ColorSpace = 0x0000_a001,
    RelatedSoundFile = 0x0000_a004,
    FlashEnergy = 0x0000_a20b,
    FocalPlaneXResolution = 0x0000_a20e,
    FocalPlaneYResolution = 0x0000_a20f,
    FocalPlaneResolutionUnit = 0x0000_a210,
    SubjectLocation = 0x0000_a214,
    ExposureIndex = 0x0000_a215,
    SensingMethod = 0x0000_a217,
    FileSource = 0x0000_a300,
    SceneType = 0x0000_a301,
    CFAPattern = 0x0000_a302,
    CustomRendered = 0x0000_a401,
    ExposureMode = 0x0000_a402,
    WhiteBalanceMode = 0x0000_a403,
    DigitalZoomRatio = 0x0000_a404,
    FocalLengthIn35mmFilm = 0x0000_a405,
    SceneCaptureType = 0x0000_a406,
    GainControl = 0x0000_a407,
    Contrast = 0x0000_a408,
    Saturation = 0x0000_a409,
    Sharpness = 0x0000_a40a,
    DeviceSettingDescription = 0x0000_a40b,
    SubjectDistanceRange = 0x0000_a40c,
    ImageUniqueID = 0x0000_a420,
    LensSpecification = 0x0000_a432,
    LensMake = 0x0000_a433,
    LensModel = 0x0000_a434,
    Gamma = 0xa500,

    GPSTimeStamp = 0x00007,
    GPSSatellites = 0x00008,
    GPSStatus = 0x00009,
    GPSMeasureMode = 0x0000a,
    GPSDOP = 0x0000b,
    GPSSpeedRef = 0x0000c,
    GPSSpeed = 0x0000d,
    GPSTrackRef = 0x0000e,
    GPSTrack = 0x0000f,
    GPSImgDirectionRef = 0x0000_0010,
    GPSImgDirection = 0x0000_0011,
    GPSMapDatum = 0x0000_0012,
    GPSDestLatitudeRef = 0x0000_0013,
    GPSDestLatitude = 0x0000_0014,
    GPSDestLongitudeRef = 0x0000_0015,
    GPSDestLongitude = 0x0000_0016,
    GPSDestBearingRef = 0x0000_0017,
    GPSDestBearing = 0x0000_0018,
    GPSDestDistanceRef = 0x0000_0019,
    GPSDestDistance = 0x0000_001a,
    GPSProcessingMethod = 0x0000_001b,
    GPSAreaInformation = 0x0000_001c,
    GPSDateStamp = 0x0000_001d,
    GPSDifferential = 0x0000_001e,
}

impl Display for ExifTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExifTag::Unknown => write!(f, "Unknown(0x{:04x})", *self as u16),
            ExifTag::Make => write!(f, "Make(0x{:04x})", *self as u16),
            ExifTag::Model => write!(f, "Model(0x{:04x})", *self as u16),
            ExifTag::Orientation => write!(f, "Orientation(0x{:04x})", *self as u16),
            ExifTag::ImageWidth => write!(f, "ImageWidth(0x{:04x})", *self as u16),
            ExifTag::ImageHeight => write!(f, "ImageHeight(0x{:04x})", *self as u16),
            ExifTag::ISOSpeedRatings => write!(f, "ISOSpeedRatings(0x{:04x})", *self as u16),
            ExifTag::ShutterSpeedValue => write!(f, "ShutterSpeedValue(0x{:04x})", *self as u16),
            ExifTag::ExposureTime => write!(f, "ExposureTime(0x{:04x})", *self as u16),
            ExifTag::FNumber => write!(f, "FNumber(0x{:04x})", *self as u16),
            ExifTag::ExifImageWidth => write!(f, "ExifImageWidth(0x{:04x})", *self as u16),
            ExifTag::ExifImageHeight => write!(f, "ExifImageHeight(0x{:04x})", *self as u16),
            ExifTag::DateTimeOriginal => write!(f, "DateTimeOriginal(0x{:04x})", *self as u16),
            ExifTag::CreateDate => write!(f, "CreateDate(0x{:04x})", *self as u16),
            ExifTag::ModifyDate => write!(f, "ModifyDate(0x{:04x})", *self as u16),
            ExifTag::OffsetTimeOriginal => write!(f, "OffsetTimeOriginal(0x{:04x})", *self as u16),
            ExifTag::OffsetTime => write!(f, "OffsetTime(0x{:04x})", *self as u16),
            ExifTag::GPSLatitudeRef => write!(f, "GPSLatitudeRef(0x{:04x})", *self as u16),
            ExifTag::GPSLatitude => write!(f, "GPSLatitude(0x{:04x})", *self as u16),
            ExifTag::GPSLongitudeRef => write!(f, "GPSLongitudeRef(0x{:04x})", *self as u16),
            ExifTag::GPSLongitude => write!(f, "GPSLongitude(0x{:04x})", *self as u16),
            ExifTag::GPSAltitudeRef => write!(f, "GPSAltitudeRef(0x{:04x})", *self as u16),
            ExifTag::GPSAltitude => write!(f, "GPSAltitude(0x{:04x})", *self as u16),
            ExifTag::GPSVersionID => write!(f, "GPSVersionID(0x{:04x})", *self as u16),
            ExifTag::ExifOffset => write!(f, "ExifOffset(0x{:04x})", *self as u16),
            ExifTag::GPSInfo => write!(f, "GPSInfo(0x{:04x})", *self as u16),
            ExifTag::ImageDescription => write!(f, "ImageDescription(0x{:04x})", *self as u16),
            ExifTag::XResolution => write!(f, "XResolution(0x{:04x})", *self as u16),
            ExifTag::YResolution => write!(f, "YResolution(0x{:04x})", *self as u16),
            ExifTag::ResolutionUnit => write!(f, "ResolutionUnit(0x{:04x})", *self as u16),
            ExifTag::Software => write!(f, "Software(0x{:04x})", *self as u16),
            ExifTag::HostComputer => write!(f, "HostComputer(0x{:04x})", *self as u16),
            ExifTag::WhitePoint => write!(f, "WhitePoint(0x{:04x})", *self as u16),
            ExifTag::PrimaryChromaticities => {
                write!(f, "PrimaryChromaticities(0x{:04x})", *self as u16)
            }
            ExifTag::YCbCrCoefficients => write!(f, "YCbCrCoefficients(0x{:04x})", *self as u16),
            ExifTag::ReferenceBlackWhite => {
                write!(f, "ReferenceBlackWhite(0x{:04x})", *self as u16)
            }
            ExifTag::Copyright => write!(f, "Copyright(0x{:04x})", *self as u16),
            ExifTag::ExposureProgram => write!(f, "ExposureProgram(0x{:04x})", *self as u16),
            ExifTag::SpectralSensitivity => {
                write!(f, "SpectralSensitivity(0x{:04x})", *self as u16)
            }
            ExifTag::OECF => write!(f, "OECF(0X{:04X})", *self as u16),
            ExifTag::SensitivityType => write!(f, "SensitivityType(0x{:04x})", *self as u16),
            ExifTag::ExifVersion => write!(f, "ExifVersion(0x{:04x})", *self as u16),
            ExifTag::ApertureValue => write!(f, "ApertureValue(0x{:04x})", *self as u16),
            ExifTag::BrightnessValue => write!(f, "BrightnessValue(0x{:04x})", *self as u16),
            ExifTag::ExposureBiasValue => write!(f, "ExposureBiasValue(0x{:04x})", *self as u16),
            ExifTag::MaxApertureValue => write!(f, "MaxApertureValue(0x{:04x})", *self as u16),
            ExifTag::SubjectDistance => write!(f, "SubjectDistance(0x{:04x})", *self as u16),
            ExifTag::MeteringMode => write!(f, "MeteringMode(0x{:04x})", *self as u16),
            ExifTag::LightSource => write!(f, "LightSource(0x{:04x})", *self as u16),
            ExifTag::Flash => write!(f, "Flash(0x{:04x})", *self as u16),
            ExifTag::FocalLength => write!(f, "FocalLength(0x{:04x})", *self as u16),
            ExifTag::SubjectArea => write!(f, "SubjectArea(0x{:04x})", *self as u16),
            ExifTag::MakerNote => write!(f, "MakerNote(0x{:04x})", *self as u16),
            ExifTag::UserComment => write!(f, "UserComment(0x{:04x})", *self as u16),
            ExifTag::FlashPixVersion => write!(f, "FlashPixVersion(0x{:04x})", *self as u16),
            ExifTag::ColorSpace => write!(f, "ColorSpace(0x{:04x})", *self as u16),
            ExifTag::RelatedSoundFile => write!(f, "RelatedSoundFile(0x{:04x})", *self as u16),
            ExifTag::FlashEnergy => write!(f, "FlashEnergy(0x{:04x})", *self as u16),
            ExifTag::FocalPlaneXResolution => {
                write!(f, "FocalPlaneXResolution(0x{:04x})", *self as u16)
            }
            ExifTag::FocalPlaneYResolution => {
                write!(f, "FocalPlaneYResolution(0x{:04x})", *self as u16)
            }
            ExifTag::FocalPlaneResolutionUnit => {
                write!(f, "FocalPlaneResolutionUnit(0x{:04x})", *self as u16)
            }
            ExifTag::SubjectLocation => write!(f, "SubjectLocation(0x{:04x})", *self as u16),
            ExifTag::ExposureIndex => write!(f, "ExposureIndex(0x{:04x})", *self as u16),
            ExifTag::SensingMethod => write!(f, "SensingMethod(0x{:04x})", *self as u16),
            ExifTag::FileSource => write!(f, "FileSource(0x{:04x})", *self as u16),
            ExifTag::SceneType => write!(f, "SceneType(0x{:04x})", *self as u16),
            ExifTag::CFAPattern => write!(f, "CFAPattern(0x{:04x})", *self as u16),
            ExifTag::CustomRendered => write!(f, "CustomRendered(0x{:04x})", *self as u16),
            ExifTag::ExposureMode => write!(f, "ExposureMode(0x{:04x})", *self as u16),
            ExifTag::WhiteBalanceMode => write!(f, "WhiteBalanceMode(0x{:04x})", *self as u16),
            ExifTag::DigitalZoomRatio => write!(f, "DigitalZoomRatio(0x{:04x})", *self as u16),
            ExifTag::FocalLengthIn35mmFilm => {
                write!(f, "FocalLengthIn35mmFilm(0x{:04x})", *self as u16)
            }
            ExifTag::SceneCaptureType => write!(f, "SceneCaptureType(0x{:04x})", *self as u16),
            ExifTag::GainControl => write!(f, "GainControl(0x{:04x})", *self as u16),
            ExifTag::Contrast => write!(f, "Contrast(0x{:04x})", *self as u16),
            ExifTag::Saturation => write!(f, "Saturation(0x{:04x})", *self as u16),
            ExifTag::Sharpness => write!(f, "Sharpness(0x{:04x})", *self as u16),
            ExifTag::DeviceSettingDescription => {
                write!(f, "DeviceSettingDescription(0x{:04x})", *self as u16)
            }
            ExifTag::SubjectDistanceRange => {
                write!(f, "SubjectDistanceRange(0x{:04x})", *self as u16)
            }
            ExifTag::ImageUniqueID => write!(f, "ImageUniqueID(0x{:04x})", *self as u16),
            ExifTag::LensSpecification => write!(f, "LensSpecification(0x{:04x})", *self as u16),
            ExifTag::LensMake => write!(f, "LensMake(0x{:04x})", *self as u16),
            ExifTag::LensModel => write!(f, "LensModel(0x{:04x})", *self as u16),
            ExifTag::Gamma => write!(f, "Gamma(0x{:04x})", *self as u16),
            ExifTag::GPSTimeStamp => write!(f, "GPSTimeStamp(0x{:04x})", *self as u16),
            ExifTag::GPSSatellites => write!(f, "GPSSatellites(0x{:04x})", *self as u16),
            ExifTag::GPSStatus => write!(f, "GPSStatus(0x{:04x})", *self as u16),
            ExifTag::GPSMeasureMode => write!(f, "GPSMeasureMode(0x{:04x})", *self as u16),
            ExifTag::GPSDOP => write!(f, "GPSDOP(0X{:04X})", *self as u16),
            ExifTag::GPSSpeedRef => write!(f, "GPSSpeedRef(0x{:04x})", *self as u16),
            ExifTag::GPSSpeed => write!(f, "GPSSpeed(0x{:04x})", *self as u16),
            ExifTag::GPSTrackRef => write!(f, "GPSTrackRef(0x{:04x})", *self as u16),
            ExifTag::GPSTrack => write!(f, "GPSTrack(0x{:04x})", *self as u16),
            ExifTag::GPSImgDirectionRef => write!(f, "GPSImgDirectionRef(0x{:04x})", *self as u16),
            ExifTag::GPSImgDirection => write!(f, "GPSImgDirection(0x{:04x})", *self as u16),
            ExifTag::GPSMapDatum => write!(f, "GPSMapDatum(0x{:04x})", *self as u16),
            ExifTag::GPSDestLatitudeRef => write!(f, "GPSDestLatitudeRef(0x{:04x})", *self as u16),
            ExifTag::GPSDestLatitude => write!(f, "GPSDestLatitude(0x{:04x})", *self as u16),
            ExifTag::GPSDestLongitudeRef => {
                write!(f, "GPSDestLongitudeRef(0x{:04x})", *self as u16)
            }
            ExifTag::GPSDestLongitude => write!(f, "GPSDestLongitude(0x{:04x})", *self as u16),
            ExifTag::GPSDestBearingRef => write!(f, "GPSDestBearingRef(0x{:04x})", *self as u16),
            ExifTag::GPSDestBearing => write!(f, "GPSDestBearing(0x{:04x})", *self as u16),
            ExifTag::GPSDestDistanceRef => write!(f, "GPSDestDistanceRef(0x{:04x})", *self as u16),
            ExifTag::GPSDestDistance => write!(f, "GPSDestDistance(0x{:04x})", *self as u16),
            ExifTag::GPSProcessingMethod => {
                write!(f, "GPSProcessingMethod(0x{:04x})", *self as u16)
            }
            ExifTag::GPSAreaInformation => write!(f, "GPSAreaInformation(0x{:04x})", *self as u16),
            ExifTag::GPSDateStamp => write!(f, "GPSDateStamp(0x{:04x})", *self as u16),
            ExifTag::GPSDifferential => write!(f, "GPSDifferential(0x{:04x})", *self as u16),
        }
    }
}

impl TryFrom<u16> for ExifTag {
    type Error = crate::Error;
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        match v {
            x if x == ExifTag::Unknown as u16 => Ok(ExifTag::Unknown),
            x if x == ExifTag::Make as u16 => Ok(ExifTag::Make),
            x if x == ExifTag::Model as u16 => Ok(ExifTag::Model),
            x if x == ExifTag::Orientation as u16 => Ok(ExifTag::Orientation),
            x if x == ExifTag::ImageWidth as u16 => Ok(ExifTag::ImageWidth),
            x if x == ExifTag::ImageHeight as u16 => Ok(ExifTag::ImageHeight),
            x if x == ExifTag::ISOSpeedRatings as u16 => Ok(ExifTag::ISOSpeedRatings),
            x if x == ExifTag::ShutterSpeedValue as u16 => Ok(ExifTag::ShutterSpeedValue),
            x if x == ExifTag::ExposureTime as u16 => Ok(ExifTag::ExposureTime),
            x if x == ExifTag::FNumber as u16 => Ok(ExifTag::FNumber),
            x if x == ExifTag::ExifImageWidth as u16 => Ok(ExifTag::ExifImageWidth),
            x if x == ExifTag::ExifImageHeight as u16 => Ok(ExifTag::ExifImageHeight),
            x if x == ExifTag::DateTimeOriginal as u16 => Ok(ExifTag::DateTimeOriginal),
            x if x == ExifTag::CreateDate as u16 => Ok(ExifTag::CreateDate),
            x if x == ExifTag::ModifyDate as u16 => Ok(ExifTag::ModifyDate),
            x if x == ExifTag::OffsetTimeOriginal as u16 => Ok(ExifTag::OffsetTimeOriginal),
            x if x == ExifTag::OffsetTime as u16 => Ok(ExifTag::OffsetTime),
            x if x == ExifTag::GPSLatitudeRef as u16 => Ok(ExifTag::GPSLatitudeRef),
            x if x == ExifTag::GPSLatitude as u16 => Ok(ExifTag::GPSLatitude),
            x if x == ExifTag::GPSLongitudeRef as u16 => Ok(ExifTag::GPSLongitudeRef),
            x if x == ExifTag::GPSLongitude as u16 => Ok(ExifTag::GPSLongitude),
            x if x == ExifTag::GPSAltitudeRef as u16 => Ok(ExifTag::GPSAltitudeRef),
            x if x == ExifTag::GPSAltitude as u16 => Ok(ExifTag::GPSAltitude),
            x if x == ExifTag::GPSVersionID as u16 => Ok(ExifTag::GPSVersionID),
            x if x == ExifTag::ExifOffset as u16 => Ok(ExifTag::ExifOffset),
            x if x == ExifTag::GPSInfo as u16 => Ok(ExifTag::GPSInfo),
            x if x == ExifTag::ImageDescription as u16 => Ok(ExifTag::ImageDescription),
            x if x == ExifTag::XResolution as u16 => Ok(ExifTag::XResolution),
            x if x == ExifTag::YResolution as u16 => Ok(ExifTag::YResolution),
            x if x == ExifTag::ResolutionUnit as u16 => Ok(ExifTag::ResolutionUnit),
            x if x == ExifTag::Software as u16 => Ok(ExifTag::Software),
            x if x == ExifTag::HostComputer as u16 => Ok(ExifTag::HostComputer),
            x if x == ExifTag::WhitePoint as u16 => Ok(ExifTag::WhitePoint),
            x if x == ExifTag::PrimaryChromaticities as u16 => Ok(ExifTag::PrimaryChromaticities),
            x if x == ExifTag::YCbCrCoefficients as u16 => Ok(ExifTag::YCbCrCoefficients),
            x if x == ExifTag::ReferenceBlackWhite as u16 => Ok(ExifTag::ReferenceBlackWhite),
            x if x == ExifTag::Copyright as u16 => Ok(ExifTag::Copyright),
            x if x == ExifTag::ExposureProgram as u16 => Ok(ExifTag::ExposureProgram),
            x if x == ExifTag::SpectralSensitivity as u16 => Ok(ExifTag::SpectralSensitivity),
            x if x == ExifTag::OECF as u16 => Ok(ExifTag::OECF),
            x if x == ExifTag::SensitivityType as u16 => Ok(ExifTag::SensitivityType),
            x if x == ExifTag::ExifVersion as u16 => Ok(ExifTag::ExifVersion),
            x if x == ExifTag::ApertureValue as u16 => Ok(ExifTag::ApertureValue),
            x if x == ExifTag::BrightnessValue as u16 => Ok(ExifTag::BrightnessValue),
            x if x == ExifTag::ExposureBiasValue as u16 => Ok(ExifTag::ExposureBiasValue),
            x if x == ExifTag::MaxApertureValue as u16 => Ok(ExifTag::MaxApertureValue),
            x if x == ExifTag::SubjectDistance as u16 => Ok(ExifTag::SubjectDistance),
            x if x == ExifTag::MeteringMode as u16 => Ok(ExifTag::MeteringMode),
            x if x == ExifTag::LightSource as u16 => Ok(ExifTag::LightSource),
            x if x == ExifTag::Flash as u16 => Ok(ExifTag::Flash),
            x if x == ExifTag::FocalLength as u16 => Ok(ExifTag::FocalLength),
            x if x == ExifTag::SubjectArea as u16 => Ok(ExifTag::SubjectArea),
            x if x == ExifTag::MakerNote as u16 => Ok(ExifTag::MakerNote),
            x if x == ExifTag::UserComment as u16 => Ok(ExifTag::UserComment),
            x if x == ExifTag::FlashPixVersion as u16 => Ok(ExifTag::FlashPixVersion),
            x if x == ExifTag::ColorSpace as u16 => Ok(ExifTag::ColorSpace),
            x if x == ExifTag::RelatedSoundFile as u16 => Ok(ExifTag::RelatedSoundFile),
            x if x == ExifTag::FlashEnergy as u16 => Ok(ExifTag::FlashEnergy),
            x if x == ExifTag::FocalPlaneXResolution as u16 => Ok(ExifTag::FocalPlaneXResolution),
            x if x == ExifTag::FocalPlaneYResolution as u16 => Ok(ExifTag::FocalPlaneYResolution),
            x if x == ExifTag::FocalPlaneResolutionUnit as u16 => {
                Ok(ExifTag::FocalPlaneResolutionUnit)
            }
            x if x == ExifTag::SubjectLocation as u16 => Ok(ExifTag::SubjectLocation),
            x if x == ExifTag::ExposureIndex as u16 => Ok(ExifTag::ExposureIndex),
            x if x == ExifTag::SensingMethod as u16 => Ok(ExifTag::SensingMethod),
            x if x == ExifTag::FileSource as u16 => Ok(ExifTag::FileSource),
            x if x == ExifTag::SceneType as u16 => Ok(ExifTag::SceneType),
            x if x == ExifTag::CFAPattern as u16 => Ok(ExifTag::CFAPattern),
            x if x == ExifTag::CustomRendered as u16 => Ok(ExifTag::CustomRendered),
            x if x == ExifTag::ExposureMode as u16 => Ok(ExifTag::ExposureMode),
            x if x == ExifTag::WhiteBalanceMode as u16 => Ok(ExifTag::WhiteBalanceMode),
            x if x == ExifTag::DigitalZoomRatio as u16 => Ok(ExifTag::DigitalZoomRatio),
            x if x == ExifTag::FocalLengthIn35mmFilm as u16 => Ok(ExifTag::FocalLengthIn35mmFilm),
            x if x == ExifTag::SceneCaptureType as u16 => Ok(ExifTag::SceneCaptureType),
            x if x == ExifTag::GainControl as u16 => Ok(ExifTag::GainControl),
            x if x == ExifTag::Contrast as u16 => Ok(ExifTag::Contrast),
            x if x == ExifTag::Saturation as u16 => Ok(ExifTag::Saturation),
            x if x == ExifTag::Sharpness as u16 => Ok(ExifTag::Sharpness),
            x if x == ExifTag::DeviceSettingDescription as u16 => {
                Ok(ExifTag::DeviceSettingDescription)
            }
            x if x == ExifTag::SubjectDistanceRange as u16 => Ok(ExifTag::SubjectDistanceRange),
            x if x == ExifTag::ImageUniqueID as u16 => Ok(ExifTag::ImageUniqueID),
            x if x == ExifTag::LensSpecification as u16 => Ok(ExifTag::LensSpecification),
            x if x == ExifTag::LensMake as u16 => Ok(ExifTag::LensMake),
            x if x == ExifTag::LensModel as u16 => Ok(ExifTag::LensModel),
            x if x == ExifTag::Gamma as u16 => Ok(ExifTag::Gamma),
            x if x == ExifTag::GPSTimeStamp as u16 => Ok(ExifTag::GPSTimeStamp),
            x if x == ExifTag::GPSSatellites as u16 => Ok(ExifTag::GPSSatellites),
            x if x == ExifTag::GPSStatus as u16 => Ok(ExifTag::GPSStatus),
            x if x == ExifTag::GPSMeasureMode as u16 => Ok(ExifTag::GPSMeasureMode),
            x if x == ExifTag::GPSDOP as u16 => Ok(ExifTag::GPSDOP),
            x if x == ExifTag::GPSSpeedRef as u16 => Ok(ExifTag::GPSSpeedRef),
            x if x == ExifTag::GPSSpeed as u16 => Ok(ExifTag::GPSSpeed),
            x if x == ExifTag::GPSTrackRef as u16 => Ok(ExifTag::GPSTrackRef),
            x if x == ExifTag::GPSTrack as u16 => Ok(ExifTag::GPSTrack),
            x if x == ExifTag::GPSImgDirectionRef as u16 => Ok(ExifTag::GPSImgDirectionRef),
            x if x == ExifTag::GPSImgDirection as u16 => Ok(ExifTag::GPSImgDirection),
            x if x == ExifTag::GPSMapDatum as u16 => Ok(ExifTag::GPSMapDatum),
            x if x == ExifTag::GPSDestLatitudeRef as u16 => Ok(ExifTag::GPSDestLatitudeRef),
            x if x == ExifTag::GPSDestLatitude as u16 => Ok(ExifTag::GPSDestLatitude),
            x if x == ExifTag::GPSDestLongitudeRef as u16 => Ok(ExifTag::GPSDestLongitudeRef),
            x if x == ExifTag::GPSDestLongitude as u16 => Ok(ExifTag::GPSDestLongitude),
            x if x == ExifTag::GPSDestBearingRef as u16 => Ok(ExifTag::GPSDestBearingRef),
            x if x == ExifTag::GPSDestBearing as u16 => Ok(ExifTag::GPSDestBearing),
            x if x == ExifTag::GPSDestDistanceRef as u16 => Ok(ExifTag::GPSDestDistanceRef),
            x if x == ExifTag::GPSDestDistance as u16 => Ok(ExifTag::GPSDestDistance),
            x if x == ExifTag::GPSProcessingMethod as u16 => Ok(ExifTag::GPSProcessingMethod),
            x if x == ExifTag::GPSAreaInformation as u16 => Ok(ExifTag::GPSAreaInformation),
            x if x == ExifTag::GPSDateStamp as u16 => Ok(ExifTag::GPSDateStamp),
            x if x == ExifTag::GPSDifferential as u16 => Ok(ExifTag::GPSDifferential),
            v => Err(format!("Unknown ExifTag 0x{v:04x}").into()),
        }
    }
}

#[allow(unused)]
pub enum Orientation {
    Horizontal,
    MirrorHorizontal,
    Rotate,
    MirrorVertical,
    MirrorHorizontalRotate270,
    Rotate90,
    MirrorHorizontalRotate90,
    Rotate270,
}

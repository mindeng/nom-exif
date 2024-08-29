//! Define exif tags and related enums, see
//! https://exiftool.org/TagNames/EXIF.html

use std::fmt::{Debug, Display};

#[cfg(feature = "json_dump")]
use serde::{Deserialize, Serialize};

#[allow(unused)]
#[cfg_attr(feature = "json_dump", derive(Serialize, Deserialize))]
#[derive(Eq, PartialEq, Hash, Clone, Copy)]
pub(crate) enum ExifTagCode {
    /// Recognized Exif tag
    Tag(ExifTag),

    /// Unrecognized Exif tag
    Code(u16),
}

impl ExifTagCode {
    /// Get recognized Exif tag, maybe return [`ExifTag::Unknown`] if it's
    /// unrecognized (you can get raw tag code via [`Self::code`] in this
    /// case).
    pub(crate) fn tag(&self) -> ExifTag {
        match self {
            ExifTagCode::Tag(t) => t.to_owned(),
            ExifTagCode::Code(_) => ExifTag::Unknown,
        }
    }

    /// Get the raw tag code value.
    pub(crate) fn code(&self) -> u16 {
        match self {
            ExifTagCode::Tag(t) => t.code(),
            ExifTagCode::Code(c) => *c,
        }
    }
}

impl From<u16> for ExifTagCode {
    fn from(v: u16) -> Self {
        let tag: crate::Result<ExifTag> = v.try_into();
        if let Ok(tag) = tag {
            ExifTagCode::Tag(tag)
        } else {
            ExifTagCode::Code(v)
        }
    }
}

impl Debug for ExifTagCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExifTagCode::Tag(t) => write!(f, "{t}"),
            ExifTagCode::Code(c) => write!(f, "Unrecognized(0x{c:04x})"),
        }
    }
}

/// Defines recognized Exif tags. Represents by [`ExifTag::Unknown`] if the tag
/// code is not recognized.
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
    OffsetTime = 0x0000_9010,
    OffsetTimeOriginal = 0x0000_9011,
    OffsetTimeDigitized = 0x0000_9012,

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

impl ExifTag {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

impl Display for ExifTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExifTag::Unknown => write!(f, "Unknown(0x{:04x})", self.code()),
            ExifTag::Make => write!(f, "Make(0x{:04x})", self.code()),
            ExifTag::Model => write!(f, "Model(0x{:04x})", self.code()),
            ExifTag::Orientation => write!(f, "Orientation(0x{:04x})", self.code()),
            ExifTag::ImageWidth => write!(f, "ImageWidth(0x{:04x})", self.code()),
            ExifTag::ImageHeight => write!(f, "ImageHeight(0x{:04x})", self.code()),
            ExifTag::ISOSpeedRatings => write!(f, "ISOSpeedRatings(0x{:04x})", self.code()),
            ExifTag::ShutterSpeedValue => write!(f, "ShutterSpeedValue(0x{:04x})", self.code()),
            ExifTag::ExposureTime => write!(f, "ExposureTime(0x{:04x})", self.code()),
            ExifTag::FNumber => write!(f, "FNumber(0x{:04x})", self.code()),
            ExifTag::ExifImageWidth => write!(f, "ExifImageWidth(0x{:04x})", self.code()),
            ExifTag::ExifImageHeight => write!(f, "ExifImageHeight(0x{:04x})", self.code()),
            ExifTag::DateTimeOriginal => write!(f, "DateTimeOriginal(0x{:04x})", self.code()),
            ExifTag::CreateDate => write!(f, "CreateDate(0x{:04x})", self.code()),
            ExifTag::ModifyDate => write!(f, "ModifyDate(0x{:04x})", self.code()),
            ExifTag::OffsetTimeOriginal => write!(f, "OffsetTimeOriginal(0x{:04x})", self.code()),
            ExifTag::OffsetTime => write!(f, "OffsetTime(0x{:04x})", self.code()),
            ExifTag::OffsetTimeDigitized => {
                write!(f, "OffsetTimeDigitized(0x{:04x})", self.code())
            }
            ExifTag::GPSLatitudeRef => write!(f, "GPSLatitudeRef(0x{:04x})", self.code()),
            ExifTag::GPSLatitude => write!(f, "GPSLatitude(0x{:04x})", self.code()),
            ExifTag::GPSLongitudeRef => write!(f, "GPSLongitudeRef(0x{:04x})", self.code()),
            ExifTag::GPSLongitude => write!(f, "GPSLongitude(0x{:04x})", self.code()),
            ExifTag::GPSAltitudeRef => write!(f, "GPSAltitudeRef(0x{:04x})", self.code()),
            ExifTag::GPSAltitude => write!(f, "GPSAltitude(0x{:04x})", self.code()),
            ExifTag::GPSVersionID => write!(f, "GPSVersionID(0x{:04x})", self.code()),
            ExifTag::ExifOffset => write!(f, "ExifOffset(0x{:04x})", self.code()),
            ExifTag::GPSInfo => write!(f, "GPSInfo(0x{:04x})", self.code()),
            ExifTag::ImageDescription => write!(f, "ImageDescription(0x{:04x})", self.code()),
            ExifTag::XResolution => write!(f, "XResolution(0x{:04x})", self.code()),
            ExifTag::YResolution => write!(f, "YResolution(0x{:04x})", self.code()),
            ExifTag::ResolutionUnit => write!(f, "ResolutionUnit(0x{:04x})", self.code()),
            ExifTag::Software => write!(f, "Software(0x{:04x})", self.code()),
            ExifTag::HostComputer => write!(f, "HostComputer(0x{:04x})", self.code()),
            ExifTag::WhitePoint => write!(f, "WhitePoint(0x{:04x})", self.code()),
            ExifTag::PrimaryChromaticities => {
                write!(f, "PrimaryChromaticities(0x{:04x})", self.code())
            }
            ExifTag::YCbCrCoefficients => write!(f, "YCbCrCoefficients(0x{:04x})", self.code()),
            ExifTag::ReferenceBlackWhite => {
                write!(f, "ReferenceBlackWhite(0x{:04x})", self.code())
            }
            ExifTag::Copyright => write!(f, "Copyright(0x{:04x})", self.code()),
            ExifTag::ExposureProgram => write!(f, "ExposureProgram(0x{:04x})", self.code()),
            ExifTag::SpectralSensitivity => {
                write!(f, "SpectralSensitivity(0x{:04x})", self.code())
            }
            ExifTag::OECF => write!(f, "OECF(0X{:04X})", self.code()),
            ExifTag::SensitivityType => write!(f, "SensitivityType(0x{:04x})", self.code()),
            ExifTag::ExifVersion => write!(f, "ExifVersion(0x{:04x})", self.code()),
            ExifTag::ApertureValue => write!(f, "ApertureValue(0x{:04x})", self.code()),
            ExifTag::BrightnessValue => write!(f, "BrightnessValue(0x{:04x})", self.code()),
            ExifTag::ExposureBiasValue => write!(f, "ExposureBiasValue(0x{:04x})", self.code()),
            ExifTag::MaxApertureValue => write!(f, "MaxApertureValue(0x{:04x})", self.code()),
            ExifTag::SubjectDistance => write!(f, "SubjectDistance(0x{:04x})", self.code()),
            ExifTag::MeteringMode => write!(f, "MeteringMode(0x{:04x})", self.code()),
            ExifTag::LightSource => write!(f, "LightSource(0x{:04x})", self.code()),
            ExifTag::Flash => write!(f, "Flash(0x{:04x})", self.code()),
            ExifTag::FocalLength => write!(f, "FocalLength(0x{:04x})", self.code()),
            ExifTag::SubjectArea => write!(f, "SubjectArea(0x{:04x})", self.code()),
            ExifTag::MakerNote => write!(f, "MakerNote(0x{:04x})", self.code()),
            ExifTag::UserComment => write!(f, "UserComment(0x{:04x})", self.code()),
            ExifTag::FlashPixVersion => write!(f, "FlashPixVersion(0x{:04x})", self.code()),
            ExifTag::ColorSpace => write!(f, "ColorSpace(0x{:04x})", self.code()),
            ExifTag::RelatedSoundFile => write!(f, "RelatedSoundFile(0x{:04x})", self.code()),
            ExifTag::FlashEnergy => write!(f, "FlashEnergy(0x{:04x})", self.code()),
            ExifTag::FocalPlaneXResolution => {
                write!(f, "FocalPlaneXResolution(0x{:04x})", self.code())
            }
            ExifTag::FocalPlaneYResolution => {
                write!(f, "FocalPlaneYResolution(0x{:04x})", self.code())
            }
            ExifTag::FocalPlaneResolutionUnit => {
                write!(f, "FocalPlaneResolutionUnit(0x{:04x})", self.code())
            }
            ExifTag::SubjectLocation => write!(f, "SubjectLocation(0x{:04x})", self.code()),
            ExifTag::ExposureIndex => write!(f, "ExposureIndex(0x{:04x})", self.code()),
            ExifTag::SensingMethod => write!(f, "SensingMethod(0x{:04x})", self.code()),
            ExifTag::FileSource => write!(f, "FileSource(0x{:04x})", self.code()),
            ExifTag::SceneType => write!(f, "SceneType(0x{:04x})", self.code()),
            ExifTag::CFAPattern => write!(f, "CFAPattern(0x{:04x})", self.code()),
            ExifTag::CustomRendered => write!(f, "CustomRendered(0x{:04x})", self.code()),
            ExifTag::ExposureMode => write!(f, "ExposureMode(0x{:04x})", self.code()),
            ExifTag::WhiteBalanceMode => write!(f, "WhiteBalanceMode(0x{:04x})", self.code()),
            ExifTag::DigitalZoomRatio => write!(f, "DigitalZoomRatio(0x{:04x})", self.code()),
            ExifTag::FocalLengthIn35mmFilm => {
                write!(f, "FocalLengthIn35mmFilm(0x{:04x})", self.code())
            }
            ExifTag::SceneCaptureType => write!(f, "SceneCaptureType(0x{:04x})", self.code()),
            ExifTag::GainControl => write!(f, "GainControl(0x{:04x})", self.code()),
            ExifTag::Contrast => write!(f, "Contrast(0x{:04x})", self.code()),
            ExifTag::Saturation => write!(f, "Saturation(0x{:04x})", self.code()),
            ExifTag::Sharpness => write!(f, "Sharpness(0x{:04x})", self.code()),
            ExifTag::DeviceSettingDescription => {
                write!(f, "DeviceSettingDescription(0x{:04x})", self.code())
            }
            ExifTag::SubjectDistanceRange => {
                write!(f, "SubjectDistanceRange(0x{:04x})", self.code())
            }
            ExifTag::ImageUniqueID => write!(f, "ImageUniqueID(0x{:04x})", self.code()),
            ExifTag::LensSpecification => write!(f, "LensSpecification(0x{:04x})", self.code()),
            ExifTag::LensMake => write!(f, "LensMake(0x{:04x})", self.code()),
            ExifTag::LensModel => write!(f, "LensModel(0x{:04x})", self.code()),
            ExifTag::Gamma => write!(f, "Gamma(0x{:04x})", self.code()),
            ExifTag::GPSTimeStamp => write!(f, "GPSTimeStamp(0x{:04x})", self.code()),
            ExifTag::GPSSatellites => write!(f, "GPSSatellites(0x{:04x})", self.code()),
            ExifTag::GPSStatus => write!(f, "GPSStatus(0x{:04x})", self.code()),
            ExifTag::GPSMeasureMode => write!(f, "GPSMeasureMode(0x{:04x})", self.code()),
            ExifTag::GPSDOP => write!(f, "GPSDOP(0X{:04X})", self.code()),
            ExifTag::GPSSpeedRef => write!(f, "GPSSpeedRef(0x{:04x})", self.code()),
            ExifTag::GPSSpeed => write!(f, "GPSSpeed(0x{:04x})", self.code()),
            ExifTag::GPSTrackRef => write!(f, "GPSTrackRef(0x{:04x})", self.code()),
            ExifTag::GPSTrack => write!(f, "GPSTrack(0x{:04x})", self.code()),
            ExifTag::GPSImgDirectionRef => write!(f, "GPSImgDirectionRef(0x{:04x})", self.code()),
            ExifTag::GPSImgDirection => write!(f, "GPSImgDirection(0x{:04x})", self.code()),
            ExifTag::GPSMapDatum => write!(f, "GPSMapDatum(0x{:04x})", self.code()),
            ExifTag::GPSDestLatitudeRef => write!(f, "GPSDestLatitudeRef(0x{:04x})", self.code()),
            ExifTag::GPSDestLatitude => write!(f, "GPSDestLatitude(0x{:04x})", self.code()),
            ExifTag::GPSDestLongitudeRef => {
                write!(f, "GPSDestLongitudeRef(0x{:04x})", self.code())
            }
            ExifTag::GPSDestLongitude => write!(f, "GPSDestLongitude(0x{:04x})", self.code()),
            ExifTag::GPSDestBearingRef => write!(f, "GPSDestBearingRef(0x{:04x})", self.code()),
            ExifTag::GPSDestBearing => write!(f, "GPSDestBearing(0x{:04x})", self.code()),
            ExifTag::GPSDestDistanceRef => write!(f, "GPSDestDistanceRef(0x{:04x})", self.code()),
            ExifTag::GPSDestDistance => write!(f, "GPSDestDistance(0x{:04x})", self.code()),
            ExifTag::GPSProcessingMethod => {
                write!(f, "GPSProcessingMethod(0x{:04x})", self.code())
            }
            ExifTag::GPSAreaInformation => write!(f, "GPSAreaInformation(0x{:04x})", self.code()),
            ExifTag::GPSDateStamp => write!(f, "GPSDateStamp(0x{:04x})", self.code()),
            ExifTag::GPSDifferential => write!(f, "GPSDifferential(0x{:04x})", self.code()),
        }
    }
}

impl TryFrom<u16> for ExifTag {
    type Error = crate::Error;
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        match v {
            x if x == ExifTag::Unknown.code() => Ok(ExifTag::Unknown),
            x if x == ExifTag::Make.code() => Ok(ExifTag::Make),
            x if x == ExifTag::Model.code() => Ok(ExifTag::Model),
            x if x == ExifTag::Orientation.code() => Ok(ExifTag::Orientation),
            x if x == ExifTag::ImageWidth.code() => Ok(ExifTag::ImageWidth),
            x if x == ExifTag::ImageHeight.code() => Ok(ExifTag::ImageHeight),
            x if x == ExifTag::ISOSpeedRatings.code() => Ok(ExifTag::ISOSpeedRatings),
            x if x == ExifTag::ShutterSpeedValue.code() => Ok(ExifTag::ShutterSpeedValue),
            x if x == ExifTag::ExposureTime.code() => Ok(ExifTag::ExposureTime),
            x if x == ExifTag::FNumber.code() => Ok(ExifTag::FNumber),
            x if x == ExifTag::ExifImageWidth.code() => Ok(ExifTag::ExifImageWidth),
            x if x == ExifTag::ExifImageHeight.code() => Ok(ExifTag::ExifImageHeight),
            x if x == ExifTag::DateTimeOriginal.code() => Ok(ExifTag::DateTimeOriginal),
            x if x == ExifTag::CreateDate.code() => Ok(ExifTag::CreateDate),
            x if x == ExifTag::ModifyDate.code() => Ok(ExifTag::ModifyDate),
            x if x == ExifTag::OffsetTimeOriginal.code() => Ok(ExifTag::OffsetTimeOriginal),
            x if x == ExifTag::OffsetTime.code() => Ok(ExifTag::OffsetTime),
            x if x == ExifTag::GPSLatitudeRef.code() => Ok(ExifTag::GPSLatitudeRef),
            x if x == ExifTag::GPSLatitude.code() => Ok(ExifTag::GPSLatitude),
            x if x == ExifTag::GPSLongitudeRef.code() => Ok(ExifTag::GPSLongitudeRef),
            x if x == ExifTag::GPSLongitude.code() => Ok(ExifTag::GPSLongitude),
            x if x == ExifTag::GPSAltitudeRef.code() => Ok(ExifTag::GPSAltitudeRef),
            x if x == ExifTag::GPSAltitude.code() => Ok(ExifTag::GPSAltitude),
            x if x == ExifTag::GPSVersionID.code() => Ok(ExifTag::GPSVersionID),
            x if x == ExifTag::ExifOffset.code() => Ok(ExifTag::ExifOffset),
            x if x == ExifTag::GPSInfo.code() => Ok(ExifTag::GPSInfo),
            x if x == ExifTag::ImageDescription.code() => Ok(ExifTag::ImageDescription),
            x if x == ExifTag::XResolution.code() => Ok(ExifTag::XResolution),
            x if x == ExifTag::YResolution.code() => Ok(ExifTag::YResolution),
            x if x == ExifTag::ResolutionUnit.code() => Ok(ExifTag::ResolutionUnit),
            x if x == ExifTag::Software.code() => Ok(ExifTag::Software),
            x if x == ExifTag::HostComputer.code() => Ok(ExifTag::HostComputer),
            x if x == ExifTag::WhitePoint.code() => Ok(ExifTag::WhitePoint),
            x if x == ExifTag::PrimaryChromaticities.code() => Ok(ExifTag::PrimaryChromaticities),
            x if x == ExifTag::YCbCrCoefficients.code() => Ok(ExifTag::YCbCrCoefficients),
            x if x == ExifTag::ReferenceBlackWhite.code() => Ok(ExifTag::ReferenceBlackWhite),
            x if x == ExifTag::Copyright.code() => Ok(ExifTag::Copyright),
            x if x == ExifTag::ExposureProgram.code() => Ok(ExifTag::ExposureProgram),
            x if x == ExifTag::SpectralSensitivity.code() => Ok(ExifTag::SpectralSensitivity),
            x if x == ExifTag::OECF.code() => Ok(ExifTag::OECF),
            x if x == ExifTag::SensitivityType.code() => Ok(ExifTag::SensitivityType),
            x if x == ExifTag::ExifVersion.code() => Ok(ExifTag::ExifVersion),
            x if x == ExifTag::ApertureValue.code() => Ok(ExifTag::ApertureValue),
            x if x == ExifTag::BrightnessValue.code() => Ok(ExifTag::BrightnessValue),
            x if x == ExifTag::ExposureBiasValue.code() => Ok(ExifTag::ExposureBiasValue),
            x if x == ExifTag::MaxApertureValue.code() => Ok(ExifTag::MaxApertureValue),
            x if x == ExifTag::SubjectDistance.code() => Ok(ExifTag::SubjectDistance),
            x if x == ExifTag::MeteringMode.code() => Ok(ExifTag::MeteringMode),
            x if x == ExifTag::LightSource.code() => Ok(ExifTag::LightSource),
            x if x == ExifTag::Flash.code() => Ok(ExifTag::Flash),
            x if x == ExifTag::FocalLength.code() => Ok(ExifTag::FocalLength),
            x if x == ExifTag::SubjectArea.code() => Ok(ExifTag::SubjectArea),
            x if x == ExifTag::MakerNote.code() => Ok(ExifTag::MakerNote),
            x if x == ExifTag::UserComment.code() => Ok(ExifTag::UserComment),
            x if x == ExifTag::FlashPixVersion.code() => Ok(ExifTag::FlashPixVersion),
            x if x == ExifTag::ColorSpace.code() => Ok(ExifTag::ColorSpace),
            x if x == ExifTag::RelatedSoundFile.code() => Ok(ExifTag::RelatedSoundFile),
            x if x == ExifTag::FlashEnergy.code() => Ok(ExifTag::FlashEnergy),
            x if x == ExifTag::FocalPlaneXResolution.code() => Ok(ExifTag::FocalPlaneXResolution),
            x if x == ExifTag::FocalPlaneYResolution.code() => Ok(ExifTag::FocalPlaneYResolution),
            x if x == ExifTag::FocalPlaneResolutionUnit.code() => {
                Ok(ExifTag::FocalPlaneResolutionUnit)
            }
            x if x == ExifTag::SubjectLocation.code() => Ok(ExifTag::SubjectLocation),
            x if x == ExifTag::ExposureIndex.code() => Ok(ExifTag::ExposureIndex),
            x if x == ExifTag::SensingMethod.code() => Ok(ExifTag::SensingMethod),
            x if x == ExifTag::FileSource.code() => Ok(ExifTag::FileSource),
            x if x == ExifTag::SceneType.code() => Ok(ExifTag::SceneType),
            x if x == ExifTag::CFAPattern.code() => Ok(ExifTag::CFAPattern),
            x if x == ExifTag::CustomRendered.code() => Ok(ExifTag::CustomRendered),
            x if x == ExifTag::ExposureMode.code() => Ok(ExifTag::ExposureMode),
            x if x == ExifTag::WhiteBalanceMode.code() => Ok(ExifTag::WhiteBalanceMode),
            x if x == ExifTag::DigitalZoomRatio.code() => Ok(ExifTag::DigitalZoomRatio),
            x if x == ExifTag::FocalLengthIn35mmFilm.code() => Ok(ExifTag::FocalLengthIn35mmFilm),
            x if x == ExifTag::SceneCaptureType.code() => Ok(ExifTag::SceneCaptureType),
            x if x == ExifTag::GainControl.code() => Ok(ExifTag::GainControl),
            x if x == ExifTag::Contrast.code() => Ok(ExifTag::Contrast),
            x if x == ExifTag::Saturation.code() => Ok(ExifTag::Saturation),
            x if x == ExifTag::Sharpness.code() => Ok(ExifTag::Sharpness),
            x if x == ExifTag::DeviceSettingDescription.code() => {
                Ok(ExifTag::DeviceSettingDescription)
            }
            x if x == ExifTag::SubjectDistanceRange.code() => Ok(ExifTag::SubjectDistanceRange),
            x if x == ExifTag::ImageUniqueID.code() => Ok(ExifTag::ImageUniqueID),
            x if x == ExifTag::LensSpecification.code() => Ok(ExifTag::LensSpecification),
            x if x == ExifTag::LensMake.code() => Ok(ExifTag::LensMake),
            x if x == ExifTag::LensModel.code() => Ok(ExifTag::LensModel),
            x if x == ExifTag::Gamma.code() => Ok(ExifTag::Gamma),
            x if x == ExifTag::GPSTimeStamp.code() => Ok(ExifTag::GPSTimeStamp),
            x if x == ExifTag::GPSSatellites.code() => Ok(ExifTag::GPSSatellites),
            x if x == ExifTag::GPSStatus.code() => Ok(ExifTag::GPSStatus),
            x if x == ExifTag::GPSMeasureMode.code() => Ok(ExifTag::GPSMeasureMode),
            x if x == ExifTag::GPSDOP.code() => Ok(ExifTag::GPSDOP),
            x if x == ExifTag::GPSSpeedRef.code() => Ok(ExifTag::GPSSpeedRef),
            x if x == ExifTag::GPSSpeed.code() => Ok(ExifTag::GPSSpeed),
            x if x == ExifTag::GPSTrackRef.code() => Ok(ExifTag::GPSTrackRef),
            x if x == ExifTag::GPSTrack.code() => Ok(ExifTag::GPSTrack),
            x if x == ExifTag::GPSImgDirectionRef.code() => Ok(ExifTag::GPSImgDirectionRef),
            x if x == ExifTag::GPSImgDirection.code() => Ok(ExifTag::GPSImgDirection),
            x if x == ExifTag::GPSMapDatum.code() => Ok(ExifTag::GPSMapDatum),
            x if x == ExifTag::GPSDestLatitudeRef.code() => Ok(ExifTag::GPSDestLatitudeRef),
            x if x == ExifTag::GPSDestLatitude.code() => Ok(ExifTag::GPSDestLatitude),
            x if x == ExifTag::GPSDestLongitudeRef.code() => Ok(ExifTag::GPSDestLongitudeRef),
            x if x == ExifTag::GPSDestLongitude.code() => Ok(ExifTag::GPSDestLongitude),
            x if x == ExifTag::GPSDestBearingRef.code() => Ok(ExifTag::GPSDestBearingRef),
            x if x == ExifTag::GPSDestBearing.code() => Ok(ExifTag::GPSDestBearing),
            x if x == ExifTag::GPSDestDistanceRef.code() => Ok(ExifTag::GPSDestDistanceRef),
            x if x == ExifTag::GPSDestDistance.code() => Ok(ExifTag::GPSDestDistance),
            x if x == ExifTag::GPSProcessingMethod.code() => Ok(ExifTag::GPSProcessingMethod),
            x if x == ExifTag::GPSAreaInformation.code() => Ok(ExifTag::GPSAreaInformation),
            x if x == ExifTag::GPSDateStamp.code() => Ok(ExifTag::GPSDateStamp),
            x if x == ExifTag::GPSDifferential.code() => Ok(ExifTag::GPSDifferential),
            v => Err(format!("Unrecognized ExifTag 0x{v:04x}").into()),
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

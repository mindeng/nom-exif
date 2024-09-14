//! Define exif tags and related enums, see
//! https://exiftool.org/TagNames/EXIF.html

use std::fmt::Display;

#[cfg(feature = "json_dump")]
use serde::{Deserialize, Serialize};

#[allow(unused)]
#[cfg_attr(feature = "json_dump", derive(Serialize, Deserialize))]
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
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
    pub(crate) fn tag(&self) -> Option<ExifTag> {
        match self {
            ExifTagCode::Tag(t) => Some(t.to_owned()),
            ExifTagCode::Code(_) => None,
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

impl Display for ExifTagCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExifTagCode::Tag(t) => t.fmt(f),
            ExifTagCode::Code(c) => format!("Unrecognized(0x{c:04x})").fmt(f),
        }
    }
}

/// Defines recognized Exif tags. All tags can be parsed, no matter if it is
/// defined here. This enum definition is just for ease of use.
///
/// You can always get the entry value by raw tag code which is an `u16` value.
/// See [`ParsedExifEntry::tag_code`](crate::ParsedExifEntry::tag_code) and
/// [`Exif::get_by_tag_code`](crate::Exif::get_by_tag_code).
#[allow(unused)]
#[cfg_attr(feature = "json_dump", derive(Serialize, Deserialize))]
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub enum ExifTag {
    Make = 0x0000_010f,
    Model = 0x0000_0110,
    Orientation = 0x0000_0112,

    ImageWidth = 0x0000_0100,
    ImageHeight = 0x0000_0101,

    ISOSpeedRatings = 0x0000_8827,
    ShutterSpeedValue = 0x0000_9201,
    ExposureTime = 0x0000_829a,
    FNumber = 0x0000_829d,

    ExifImageWidth = 0x0000_a002,
    ExifImageHeight = 0x0000_a003,

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
    Gamma = 0x0000_a500,

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

    YCbCrPositioning = 0x0000_0213,
    RecommendedExposureIndex = 0x0000_8832,
    SubSecTimeDigitized = 0x0000_9292,
    SubSecTimeOriginal = 0x0000_9291,
    SubSecTime = 0x0000_9290,
    InteropOffset = 0x0000_a005,
    ComponentsConfiguration = 0x0000_9101,
    ThumbnailOffset = 0x0000_0201,
    ThumbnailLength = 0x0000_0202,
    Compression = 0x0000_0103,
    BitsPerSample = 0x0000_0102,
    PhotometricInterpretation = 0x0000_0106,
    SamplesPerPixel = 0x0000_0115,
    RowsPerStrip = 0x0000_0116,
    PlanarConfiguration = 0x0000_011c,
}

impl ExifTag {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

impl Display for ExifTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &str = (*self).into();
        s.fmt(f)
    }
}

impl From<ExifTag> for &str {
    fn from(value: ExifTag) -> Self {
        match value {
            ExifTag::Make => "Make",
            ExifTag::Model => "Model",
            ExifTag::Orientation => "Orientation",
            ExifTag::ImageWidth => "ImageWidth",
            ExifTag::ImageHeight => "ImageHeight",
            ExifTag::ISOSpeedRatings => "ISOSpeedRatings",
            ExifTag::ShutterSpeedValue => "ShutterSpeedValue",
            ExifTag::ExposureTime => "ExposureTime",
            ExifTag::FNumber => "FNumber",
            ExifTag::ExifImageWidth => "ExifImageWidth",
            ExifTag::ExifImageHeight => "ExifImageHeight",
            ExifTag::DateTimeOriginal => "DateTimeOriginal",
            ExifTag::CreateDate => "CreateDate",
            ExifTag::ModifyDate => "ModifyDate",
            ExifTag::OffsetTime => "OffsetTime",
            ExifTag::OffsetTimeOriginal => "OffsetTimeOriginal",
            ExifTag::OffsetTimeDigitized => "OffsetTimeDigitized",
            ExifTag::GPSLatitudeRef => "GPSLatitudeRef",
            ExifTag::GPSLatitude => "GPSLatitude",
            ExifTag::GPSLongitudeRef => "GPSLongitudeRef",
            ExifTag::GPSLongitude => "GPSLongitude",
            ExifTag::GPSAltitudeRef => "GPSAltitudeRef",
            ExifTag::GPSAltitude => "GPSAltitude",
            ExifTag::GPSVersionID => "GPSVersionID",
            ExifTag::ExifOffset => "ExifOffset",
            ExifTag::GPSInfo => "GPSInfo",
            ExifTag::ImageDescription => "ImageDescription",
            ExifTag::XResolution => "XResolution",
            ExifTag::YResolution => "YResolution",
            ExifTag::ResolutionUnit => "ResolutionUnit",
            ExifTag::Software => "Software",
            ExifTag::HostComputer => "HostComputer",
            ExifTag::WhitePoint => "WhitePoint",
            ExifTag::PrimaryChromaticities => "PrimaryChromaticities",
            ExifTag::YCbCrCoefficients => "YCbCrCoefficients",
            ExifTag::ReferenceBlackWhite => "ReferenceBlackWhite",
            ExifTag::Copyright => "Copyright",
            ExifTag::ExposureProgram => "ExposureProgram",
            ExifTag::SpectralSensitivity => "SpectralSensitivity",
            ExifTag::OECF => "OECF",
            ExifTag::SensitivityType => "SensitivityType",
            ExifTag::ExifVersion => "ExifVersion",
            ExifTag::ApertureValue => "ApertureValue",
            ExifTag::BrightnessValue => "BrightnessValue",
            ExifTag::ExposureBiasValue => "ExposureBiasValue",
            ExifTag::MaxApertureValue => "MaxApertureValue",
            ExifTag::SubjectDistance => "SubjectDistance",
            ExifTag::MeteringMode => "MeteringMode",
            ExifTag::LightSource => "LightSource",
            ExifTag::Flash => "Flash",
            ExifTag::FocalLength => "FocalLength",
            ExifTag::SubjectArea => "SubjectArea",
            ExifTag::MakerNote => "MakerNote",
            ExifTag::UserComment => "UserComment",
            ExifTag::FlashPixVersion => "FlashPixVersion",
            ExifTag::ColorSpace => "ColorSpace",
            ExifTag::RelatedSoundFile => "RelatedSoundFile",
            ExifTag::FlashEnergy => "FlashEnergy",
            ExifTag::FocalPlaneXResolution => "FocalPlaneXResolution",
            ExifTag::FocalPlaneYResolution => "FocalPlaneYResolution",
            ExifTag::FocalPlaneResolutionUnit => "FocalPlaneResolutionUnit",
            ExifTag::SubjectLocation => "SubjectLocation",
            ExifTag::ExposureIndex => "ExposureIndex",
            ExifTag::SensingMethod => "SensingMethod",
            ExifTag::FileSource => "FileSource",
            ExifTag::SceneType => "SceneType",
            ExifTag::CFAPattern => "CFAPattern",
            ExifTag::CustomRendered => "CustomRendered",
            ExifTag::ExposureMode => "ExposureMode",
            ExifTag::WhiteBalanceMode => "WhiteBalanceMode",
            ExifTag::DigitalZoomRatio => "DigitalZoomRatio",
            ExifTag::FocalLengthIn35mmFilm => "FocalLengthIn35mmFilm",
            ExifTag::SceneCaptureType => "SceneCaptureType",
            ExifTag::GainControl => "GainControl",
            ExifTag::Contrast => "Contrast",
            ExifTag::Saturation => "Saturation",
            ExifTag::Sharpness => "Sharpness",
            ExifTag::DeviceSettingDescription => "DeviceSettingDescription",
            ExifTag::SubjectDistanceRange => "SubjectDistanceRange",
            ExifTag::ImageUniqueID => "ImageUniqueID",
            ExifTag::LensSpecification => "LensSpecification",
            ExifTag::LensMake => "LensMake",
            ExifTag::LensModel => "LensModel",
            ExifTag::Gamma => "Gamma",
            ExifTag::GPSTimeStamp => "GPSTimeStamp",
            ExifTag::GPSSatellites => "GPSSatellites",
            ExifTag::GPSStatus => "GPSStatus",
            ExifTag::GPSMeasureMode => "GPSMeasureMode",
            ExifTag::GPSDOP => "GPSDOP",
            ExifTag::GPSSpeedRef => "GPSSpeedRef",
            ExifTag::GPSSpeed => "GPSSpeed",
            ExifTag::GPSTrackRef => "GPSTrackRef",
            ExifTag::GPSTrack => "GPSTrack",
            ExifTag::GPSImgDirectionRef => "GPSImgDirectionRef",
            ExifTag::GPSImgDirection => "GPSImgDirection",
            ExifTag::GPSMapDatum => "GPSMapDatum",
            ExifTag::GPSDestLatitudeRef => "GPSDestLatitudeRef",
            ExifTag::GPSDestLatitude => "GPSDestLatitude",
            ExifTag::GPSDestLongitudeRef => "GPSDestLongitudeRef",
            ExifTag::GPSDestLongitude => "GPSDestLongitude",
            ExifTag::GPSDestBearingRef => "GPSDestBearingRef",
            ExifTag::GPSDestBearing => "GPSDestBearing",
            ExifTag::GPSDestDistanceRef => "GPSDestDistanceRef",
            ExifTag::GPSDestDistance => "GPSDestDistance",
            ExifTag::GPSProcessingMethod => "GPSProcessingMethod",
            ExifTag::GPSAreaInformation => "GPSAreaInformation",
            ExifTag::GPSDateStamp => "GPSDateStamp",
            ExifTag::GPSDifferential => "GPSDifferential",
            ExifTag::YCbCrPositioning => "YCbCrPositioning",
            ExifTag::RecommendedExposureIndex => "RecommendedExposureIndex",
            ExifTag::SubSecTimeDigitized => "SubSecTimeDigitized",
            ExifTag::SubSecTimeOriginal => "SubSecTimeOriginal",
            ExifTag::SubSecTime => "SubSecTime",
            ExifTag::InteropOffset => "InteropOffset",
            ExifTag::ComponentsConfiguration => "ComponentsConfiguration",
            ExifTag::ThumbnailOffset => "ThumbnailOffset",
            ExifTag::ThumbnailLength => "ThumbnailLength",
            ExifTag::Compression => "Compression",
            ExifTag::BitsPerSample => "BitsPerSample",
            ExifTag::PhotometricInterpretation => "PhotometricInterpretation",
            ExifTag::SamplesPerPixel => "SamplesPerPixel",
            ExifTag::RowsPerStrip => "RowsPerStrip",
            ExifTag::PlanarConfiguration => "PlanarConfiguration",
        }
    }
}

impl TryFrom<u16> for ExifTag {
    type Error = crate::Error;
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        use ExifTag::*;

        let tag = match v {
            x if x == Make.code() => Self::Make,
            x if x == Model.code() => Self::Model,
            x if x == Orientation.code() => Self::Orientation,
            x if x == ImageWidth.code() => Self::ImageWidth,
            x if x == ImageHeight.code() => Self::ImageHeight,
            x if x == ISOSpeedRatings.code() => Self::ISOSpeedRatings,
            x if x == ShutterSpeedValue.code() => Self::ShutterSpeedValue,
            x if x == ExposureTime.code() => Self::ExposureTime,
            x if x == FNumber.code() => Self::FNumber,
            x if x == ExifImageWidth.code() => Self::ExifImageWidth,
            x if x == ExifImageHeight.code() => Self::ExifImageHeight,
            x if x == DateTimeOriginal.code() => Self::DateTimeOriginal,
            x if x == CreateDate.code() => Self::CreateDate,
            x if x == ModifyDate.code() => Self::ModifyDate,
            x if x == OffsetTime.code() => Self::OffsetTime,
            x if x == OffsetTimeOriginal.code() => Self::OffsetTimeOriginal,
            x if x == OffsetTimeDigitized.code() => Self::OffsetTimeDigitized,
            x if x == GPSLatitudeRef.code() => Self::GPSLatitudeRef,
            x if x == GPSLatitude.code() => Self::GPSLatitude,
            x if x == GPSLongitudeRef.code() => Self::GPSLongitudeRef,
            x if x == GPSLongitude.code() => Self::GPSLongitude,
            x if x == GPSAltitudeRef.code() => Self::GPSAltitudeRef,
            x if x == GPSAltitude.code() => Self::GPSAltitude,
            x if x == GPSVersionID.code() => Self::GPSVersionID,
            x if x == ExifOffset.code() => Self::ExifOffset,
            x if x == GPSInfo.code() => Self::GPSInfo,
            x if x == ImageDescription.code() => Self::ImageDescription,
            x if x == XResolution.code() => Self::XResolution,
            x if x == YResolution.code() => Self::YResolution,
            x if x == ResolutionUnit.code() => Self::ResolutionUnit,
            x if x == Software.code() => Self::Software,
            x if x == HostComputer.code() => Self::HostComputer,
            x if x == WhitePoint.code() => Self::WhitePoint,
            x if x == PrimaryChromaticities.code() => Self::PrimaryChromaticities,
            x if x == YCbCrCoefficients.code() => Self::YCbCrCoefficients,
            x if x == ReferenceBlackWhite.code() => Self::ReferenceBlackWhite,
            x if x == Copyright.code() => Self::Copyright,
            x if x == ExposureProgram.code() => Self::ExposureProgram,
            x if x == SpectralSensitivity.code() => Self::SpectralSensitivity,
            x if x == OECF.code() => Self::OECF,
            x if x == SensitivityType.code() => Self::SensitivityType,
            x if x == ExifVersion.code() => Self::ExifVersion,
            x if x == ApertureValue.code() => Self::ApertureValue,
            x if x == BrightnessValue.code() => Self::BrightnessValue,
            x if x == ExposureBiasValue.code() => Self::ExposureBiasValue,
            x if x == MaxApertureValue.code() => Self::MaxApertureValue,
            x if x == SubjectDistance.code() => Self::SubjectDistance,
            x if x == MeteringMode.code() => Self::MeteringMode,
            x if x == LightSource.code() => Self::LightSource,
            x if x == Flash.code() => Self::Flash,
            x if x == FocalLength.code() => Self::FocalLength,
            x if x == SubjectArea.code() => Self::SubjectArea,
            x if x == MakerNote.code() => Self::MakerNote,
            x if x == UserComment.code() => Self::UserComment,
            x if x == FlashPixVersion.code() => Self::FlashPixVersion,
            x if x == ColorSpace.code() => Self::ColorSpace,
            x if x == RelatedSoundFile.code() => Self::RelatedSoundFile,
            x if x == FlashEnergy.code() => Self::FlashEnergy,
            x if x == FocalPlaneXResolution.code() => Self::FocalPlaneXResolution,
            x if x == FocalPlaneYResolution.code() => Self::FocalPlaneYResolution,
            x if x == FocalPlaneResolutionUnit.code() => Self::FocalPlaneResolutionUnit,
            x if x == SubjectLocation.code() => Self::SubjectLocation,
            x if x == ExposureIndex.code() => Self::ExposureIndex,
            x if x == SensingMethod.code() => Self::SensingMethod,
            x if x == FileSource.code() => Self::FileSource,
            x if x == SceneType.code() => Self::SceneType,
            x if x == CFAPattern.code() => Self::CFAPattern,
            x if x == CustomRendered.code() => Self::CustomRendered,
            x if x == ExposureMode.code() => Self::ExposureMode,
            x if x == WhiteBalanceMode.code() => Self::WhiteBalanceMode,
            x if x == DigitalZoomRatio.code() => Self::DigitalZoomRatio,
            x if x == FocalLengthIn35mmFilm.code() => Self::FocalLengthIn35mmFilm,
            x if x == SceneCaptureType.code() => Self::SceneCaptureType,
            x if x == GainControl.code() => Self::GainControl,
            x if x == Contrast.code() => Self::Contrast,
            x if x == Saturation.code() => Self::Saturation,
            x if x == Sharpness.code() => Self::Sharpness,
            x if x == DeviceSettingDescription.code() => Self::DeviceSettingDescription,
            x if x == SubjectDistanceRange.code() => Self::SubjectDistanceRange,
            x if x == ImageUniqueID.code() => Self::ImageUniqueID,
            x if x == LensSpecification.code() => Self::LensSpecification,
            x if x == LensMake.code() => Self::LensMake,
            x if x == LensModel.code() => Self::LensModel,
            x if x == Gamma.code() => Self::Gamma,
            x if x == GPSTimeStamp.code() => Self::GPSTimeStamp,
            x if x == GPSSatellites.code() => Self::GPSSatellites,
            x if x == GPSStatus.code() => Self::GPSStatus,
            x if x == GPSMeasureMode.code() => Self::GPSMeasureMode,
            x if x == GPSDOP.code() => Self::GPSDOP,
            x if x == GPSSpeedRef.code() => Self::GPSSpeedRef,
            x if x == GPSSpeed.code() => Self::GPSSpeed,
            x if x == GPSTrackRef.code() => Self::GPSTrackRef,
            x if x == GPSTrack.code() => Self::GPSTrack,
            x if x == GPSImgDirectionRef.code() => Self::GPSImgDirectionRef,
            x if x == GPSImgDirection.code() => Self::GPSImgDirection,
            x if x == GPSMapDatum.code() => Self::GPSMapDatum,
            x if x == GPSDestLatitudeRef.code() => Self::GPSDestLatitudeRef,
            x if x == GPSDestLatitude.code() => Self::GPSDestLatitude,
            x if x == GPSDestLongitudeRef.code() => Self::GPSDestLongitudeRef,
            x if x == GPSDestLongitude.code() => Self::GPSDestLongitude,
            x if x == GPSDestBearingRef.code() => Self::GPSDestBearingRef,
            x if x == GPSDestBearing.code() => Self::GPSDestBearing,
            x if x == GPSDestDistanceRef.code() => Self::GPSDestDistanceRef,
            x if x == GPSDestDistance.code() => Self::GPSDestDistance,
            x if x == GPSProcessingMethod.code() => Self::GPSProcessingMethod,
            x if x == GPSAreaInformation.code() => Self::GPSAreaInformation,
            x if x == GPSDateStamp.code() => Self::GPSDateStamp,
            x if x == GPSDifferential.code() => Self::GPSDifferential,
            x if x == YCbCrPositioning.code() => Self::YCbCrPositioning,
            x if x == RecommendedExposureIndex.code() => Self::RecommendedExposureIndex,
            x if x == SubSecTimeDigitized.code() => Self::SubSecTimeDigitized,
            x if x == SubSecTimeOriginal.code() => Self::SubSecTimeOriginal,
            x if x == SubSecTime.code() => Self::SubSecTime,
            x if x == InteropOffset.code() => Self::InteropOffset,
            x if x == ComponentsConfiguration.code() => Self::ComponentsConfiguration,
            x if x == ThumbnailOffset.code() => Self::ThumbnailOffset,
            x if x == ThumbnailLength.code() => Self::ThumbnailLength,
            x if x == Compression.code() => Self::Compression,
            x if x == BitsPerSample.code() => Self::BitsPerSample,
            x if x == PhotometricInterpretation.code() => Self::PhotometricInterpretation,
            x if x == SamplesPerPixel.code() => Self::SamplesPerPixel,
            x if x == RowsPerStrip.code() => Self::RowsPerStrip,
            x if x == PlanarConfiguration.code() => Self::PlanarConfiguration,

            o => return Err(format!("Unrecognized ExifTag 0x{o:04x}").into()),
        };

        Ok(tag)
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

//! Define exif tags and related enums, see
//! https://exiftool.org/TagNames/EXIF.html

use core::fmt::{Debug, Display};

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
    pub(crate) const fn tag(&self) -> Option<ExifTag> {
        match *self {
            Self::Tag(t) => Some(t),
            Self::Code(_) => None,
        }
    }

    /// Get the raw tag code value.
    pub(crate) const fn code(&self) -> u16 {
        match *self {
            Self::Tag(t) => t.code(),
            Self::Code(c) => c,
        }
    }
}

impl From<u16> for ExifTagCode {
    fn from(v: u16) -> Self {
        let tag: crate::Result<ExifTag> = v.try_into();
        tag.map_or(Self::Code(v), Self::Tag)
    }
}

impl Debug for ExifTagCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Tag(t) => write!(f, "{t}"),
            Self::Code(c) => write!(f, "Unrecognized(0x{c:04x})"),
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
    /// `Unknown` has been deprecated, please don't use this variant in your
    /// code (use "_" to ommit it if you are using match statement).
    ///
    /// The parser won't return this variant anymore. It will be deleted in
    /// next major version.
    #[deprecated(since = "1.5.0", note = "won't return this variant anymore")]
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
    #[inline]
    pub const fn code(self) -> u16 {
        self as u16
    }
}

impl Display for ExifTag {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[allow(deprecated)]
            Self::Unknown => write!(f, "Unknown(0x{:04x})", self.code()),
            Self::Make => write!(f, "Make(0x{:04x})", self.code()),
            Self::Model => write!(f, "Model(0x{:04x})", self.code()),
            Self::Orientation => write!(f, "Orientation(0x{:04x})", self.code()),
            Self::ImageWidth => write!(f, "ImageWidth(0x{:04x})", self.code()),
            Self::ImageHeight => write!(f, "ImageHeight(0x{:04x})", self.code()),
            Self::ISOSpeedRatings => write!(f, "ISOSpeedRatings(0x{:04x})", self.code()),
            Self::ShutterSpeedValue => write!(f, "ShutterSpeedValue(0x{:04x})", self.code()),
            Self::ExposureTime => write!(f, "ExposureTime(0x{:04x})", self.code()),
            Self::FNumber => write!(f, "FNumber(0x{:04x})", self.code()),
            Self::ExifImageWidth => write!(f, "ExifImageWidth(0x{:04x})", self.code()),
            Self::ExifImageHeight => write!(f, "ExifImageHeight(0x{:04x})", self.code()),
            Self::DateTimeOriginal => write!(f, "DateTimeOriginal(0x{:04x})", self.code()),
            Self::CreateDate => write!(f, "CreateDate(0x{:04x})", self.code()),
            Self::ModifyDate => write!(f, "ModifyDate(0x{:04x})", self.code()),
            Self::OffsetTimeOriginal => write!(f, "OffsetTimeOriginal(0x{:04x})", self.code()),
            Self::OffsetTime => write!(f, "OffsetTime(0x{:04x})", self.code()),
            Self::OffsetTimeDigitized => {
                write!(f, "OffsetTimeDigitized(0x{:04x})", self.code())
            }
            Self::GPSLatitudeRef => write!(f, "GPSLatitudeRef(0x{:04x})", self.code()),
            Self::GPSLatitude => write!(f, "GPSLatitude(0x{:04x})", self.code()),
            Self::GPSLongitudeRef => write!(f, "GPSLongitudeRef(0x{:04x})", self.code()),
            Self::GPSLongitude => write!(f, "GPSLongitude(0x{:04x})", self.code()),
            Self::GPSAltitudeRef => write!(f, "GPSAltitudeRef(0x{:04x})", self.code()),
            Self::GPSAltitude => write!(f, "GPSAltitude(0x{:04x})", self.code()),
            Self::GPSVersionID => write!(f, "GPSVersionID(0x{:04x})", self.code()),
            Self::ExifOffset => write!(f, "ExifOffset(0x{:04x})", self.code()),
            Self::GPSInfo => write!(f, "GPSInfo(0x{:04x})", self.code()),
            Self::ImageDescription => write!(f, "ImageDescription(0x{:04x})", self.code()),
            Self::XResolution => write!(f, "XResolution(0x{:04x})", self.code()),
            Self::YResolution => write!(f, "YResolution(0x{:04x})", self.code()),
            Self::ResolutionUnit => write!(f, "ResolutionUnit(0x{:04x})", self.code()),
            Self::Software => write!(f, "Software(0x{:04x})", self.code()),
            Self::HostComputer => write!(f, "HostComputer(0x{:04x})", self.code()),
            Self::WhitePoint => write!(f, "WhitePoint(0x{:04x})", self.code()),
            Self::PrimaryChromaticities => {
                write!(f, "PrimaryChromaticities(0x{:04x})", self.code())
            }
            Self::YCbCrCoefficients => write!(f, "YCbCrCoefficients(0x{:04x})", self.code()),
            Self::ReferenceBlackWhite => {
                write!(f, "ReferenceBlackWhite(0x{:04x})", self.code())
            }
            Self::Copyright => write!(f, "Copyright(0x{:04x})", self.code()),
            Self::ExposureProgram => write!(f, "ExposureProgram(0x{:04x})", self.code()),
            Self::SpectralSensitivity => {
                write!(f, "SpectralSensitivity(0x{:04x})", self.code())
            }
            Self::OECF => write!(f, "OECF(0X{:04X})", self.code()),
            Self::SensitivityType => write!(f, "SensitivityType(0x{:04x})", self.code()),
            Self::ExifVersion => write!(f, "ExifVersion(0x{:04x})", self.code()),
            Self::ApertureValue => write!(f, "ApertureValue(0x{:04x})", self.code()),
            Self::BrightnessValue => write!(f, "BrightnessValue(0x{:04x})", self.code()),
            Self::ExposureBiasValue => write!(f, "ExposureBiasValue(0x{:04x})", self.code()),
            Self::MaxApertureValue => write!(f, "MaxApertureValue(0x{:04x})", self.code()),
            Self::SubjectDistance => write!(f, "SubjectDistance(0x{:04x})", self.code()),
            Self::MeteringMode => write!(f, "MeteringMode(0x{:04x})", self.code()),
            Self::LightSource => write!(f, "LightSource(0x{:04x})", self.code()),
            Self::Flash => write!(f, "Flash(0x{:04x})", self.code()),
            Self::FocalLength => write!(f, "FocalLength(0x{:04x})", self.code()),
            Self::SubjectArea => write!(f, "SubjectArea(0x{:04x})", self.code()),
            Self::MakerNote => write!(f, "MakerNote(0x{:04x})", self.code()),
            Self::UserComment => write!(f, "UserComment(0x{:04x})", self.code()),
            Self::FlashPixVersion => write!(f, "FlashPixVersion(0x{:04x})", self.code()),
            Self::ColorSpace => write!(f, "ColorSpace(0x{:04x})", self.code()),
            Self::RelatedSoundFile => write!(f, "RelatedSoundFile(0x{:04x})", self.code()),
            Self::FlashEnergy => write!(f, "FlashEnergy(0x{:04x})", self.code()),
            Self::FocalPlaneXResolution => {
                write!(f, "FocalPlaneXResolution(0x{:04x})", self.code())
            }
            Self::FocalPlaneYResolution => {
                write!(f, "FocalPlaneYResolution(0x{:04x})", self.code())
            }
            Self::FocalPlaneResolutionUnit => {
                write!(f, "FocalPlaneResolutionUnit(0x{:04x})", self.code())
            }
            Self::SubjectLocation => write!(f, "SubjectLocation(0x{:04x})", self.code()),
            Self::ExposureIndex => write!(f, "ExposureIndex(0x{:04x})", self.code()),
            Self::SensingMethod => write!(f, "SensingMethod(0x{:04x})", self.code()),
            Self::FileSource => write!(f, "FileSource(0x{:04x})", self.code()),
            Self::SceneType => write!(f, "SceneType(0x{:04x})", self.code()),
            Self::CFAPattern => write!(f, "CFAPattern(0x{:04x})", self.code()),
            Self::CustomRendered => write!(f, "CustomRendered(0x{:04x})", self.code()),
            Self::ExposureMode => write!(f, "ExposureMode(0x{:04x})", self.code()),
            Self::WhiteBalanceMode => write!(f, "WhiteBalanceMode(0x{:04x})", self.code()),
            Self::DigitalZoomRatio => write!(f, "DigitalZoomRatio(0x{:04x})", self.code()),
            Self::FocalLengthIn35mmFilm => {
                write!(f, "FocalLengthIn35mmFilm(0x{:04x})", self.code())
            }
            Self::SceneCaptureType => write!(f, "SceneCaptureType(0x{:04x})", self.code()),
            Self::GainControl => write!(f, "GainControl(0x{:04x})", self.code()),
            Self::Contrast => write!(f, "Contrast(0x{:04x})", self.code()),
            Self::Saturation => write!(f, "Saturation(0x{:04x})", self.code()),
            Self::Sharpness => write!(f, "Sharpness(0x{:04x})", self.code()),
            Self::DeviceSettingDescription => {
                write!(f, "DeviceSettingDescription(0x{:04x})", self.code())
            }
            Self::SubjectDistanceRange => {
                write!(f, "SubjectDistanceRange(0x{:04x})", self.code())
            }
            Self::ImageUniqueID => write!(f, "ImageUniqueID(0x{:04x})", self.code()),
            Self::LensSpecification => write!(f, "LensSpecification(0x{:04x})", self.code()),
            Self::LensMake => write!(f, "LensMake(0x{:04x})", self.code()),
            Self::LensModel => write!(f, "LensModel(0x{:04x})", self.code()),
            Self::Gamma => write!(f, "Gamma(0x{:04x})", self.code()),
            Self::GPSTimeStamp => write!(f, "GPSTimeStamp(0x{:04x})", self.code()),
            Self::GPSSatellites => write!(f, "GPSSatellites(0x{:04x})", self.code()),
            Self::GPSStatus => write!(f, "GPSStatus(0x{:04x})", self.code()),
            Self::GPSMeasureMode => write!(f, "GPSMeasureMode(0x{:04x})", self.code()),
            Self::GPSDOP => write!(f, "GPSDOP(0X{:04X})", self.code()),
            Self::GPSSpeedRef => write!(f, "GPSSpeedRef(0x{:04x})", self.code()),
            Self::GPSSpeed => write!(f, "GPSSpeed(0x{:04x})", self.code()),
            Self::GPSTrackRef => write!(f, "GPSTrackRef(0x{:04x})", self.code()),
            Self::GPSTrack => write!(f, "GPSTrack(0x{:04x})", self.code()),
            Self::GPSImgDirectionRef => write!(f, "GPSImgDirectionRef(0x{:04x})", self.code()),
            Self::GPSImgDirection => write!(f, "GPSImgDirection(0x{:04x})", self.code()),
            Self::GPSMapDatum => write!(f, "GPSMapDatum(0x{:04x})", self.code()),
            Self::GPSDestLatitudeRef => write!(f, "GPSDestLatitudeRef(0x{:04x})", self.code()),
            Self::GPSDestLatitude => write!(f, "GPSDestLatitude(0x{:04x})", self.code()),
            Self::GPSDestLongitudeRef => {
                write!(f, "GPSDestLongitudeRef(0x{:04x})", self.code())
            }
            Self::GPSDestLongitude => write!(f, "GPSDestLongitude(0x{:04x})", self.code()),
            Self::GPSDestBearingRef => write!(f, "GPSDestBearingRef(0x{:04x})", self.code()),
            Self::GPSDestBearing => write!(f, "GPSDestBearing(0x{:04x})", self.code()),
            Self::GPSDestDistanceRef => write!(f, "GPSDestDistanceRef(0x{:04x})", self.code()),
            Self::GPSDestDistance => write!(f, "GPSDestDistance(0x{:04x})", self.code()),
            Self::GPSProcessingMethod => {
                write!(f, "GPSProcessingMethod(0x{:04x})", self.code())
            }
            Self::GPSAreaInformation => write!(f, "GPSAreaInformation(0x{:04x})", self.code()),
            Self::GPSDateStamp => write!(f, "GPSDateStamp(0x{:04x})", self.code()),
            Self::GPSDifferential => write!(f, "GPSDifferential(0x{:04x})", self.code()),
        }
    }
}

impl TryFrom<u16> for ExifTag {
    type Error = crate::Error;

    #[inline]
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        match v {
            #[allow(deprecated)]
            x if x == Self::Unknown.code() => Ok(Self::Unknown),
            x if x == Self::Make.code() => Ok(Self::Make),
            x if x == Self::Model.code() => Ok(Self::Model),
            x if x == Self::Orientation.code() => Ok(Self::Orientation),
            x if x == Self::ImageWidth.code() => Ok(Self::ImageWidth),
            x if x == Self::ImageHeight.code() => Ok(Self::ImageHeight),
            x if x == Self::ISOSpeedRatings.code() => Ok(Self::ISOSpeedRatings),
            x if x == Self::ShutterSpeedValue.code() => Ok(Self::ShutterSpeedValue),
            x if x == Self::ExposureTime.code() => Ok(Self::ExposureTime),
            x if x == Self::FNumber.code() => Ok(Self::FNumber),
            x if x == Self::ExifImageWidth.code() => Ok(Self::ExifImageWidth),
            x if x == Self::ExifImageHeight.code() => Ok(Self::ExifImageHeight),
            x if x == Self::DateTimeOriginal.code() => Ok(Self::DateTimeOriginal),
            x if x == Self::CreateDate.code() => Ok(Self::CreateDate),
            x if x == Self::ModifyDate.code() => Ok(Self::ModifyDate),
            x if x == Self::OffsetTimeOriginal.code() => Ok(Self::OffsetTimeOriginal),
            x if x == Self::OffsetTime.code() => Ok(Self::OffsetTime),
            x if x == Self::GPSLatitudeRef.code() => Ok(Self::GPSLatitudeRef),
            x if x == Self::GPSLatitude.code() => Ok(Self::GPSLatitude),
            x if x == Self::GPSLongitudeRef.code() => Ok(Self::GPSLongitudeRef),
            x if x == Self::GPSLongitude.code() => Ok(Self::GPSLongitude),
            x if x == Self::GPSAltitudeRef.code() => Ok(Self::GPSAltitudeRef),
            x if x == Self::GPSAltitude.code() => Ok(Self::GPSAltitude),
            x if x == Self::GPSVersionID.code() => Ok(Self::GPSVersionID),
            x if x == Self::ExifOffset.code() => Ok(Self::ExifOffset),
            x if x == Self::GPSInfo.code() => Ok(Self::GPSInfo),
            x if x == Self::ImageDescription.code() => Ok(Self::ImageDescription),
            x if x == Self::XResolution.code() => Ok(Self::XResolution),
            x if x == Self::YResolution.code() => Ok(Self::YResolution),
            x if x == Self::ResolutionUnit.code() => Ok(Self::ResolutionUnit),
            x if x == Self::Software.code() => Ok(Self::Software),
            x if x == Self::HostComputer.code() => Ok(Self::HostComputer),
            x if x == Self::WhitePoint.code() => Ok(Self::WhitePoint),
            x if x == Self::PrimaryChromaticities.code() => Ok(Self::PrimaryChromaticities),
            x if x == Self::YCbCrCoefficients.code() => Ok(Self::YCbCrCoefficients),
            x if x == Self::ReferenceBlackWhite.code() => Ok(Self::ReferenceBlackWhite),
            x if x == Self::Copyright.code() => Ok(Self::Copyright),
            x if x == Self::ExposureProgram.code() => Ok(Self::ExposureProgram),
            x if x == Self::SpectralSensitivity.code() => Ok(Self::SpectralSensitivity),
            x if x == Self::OECF.code() => Ok(Self::OECF),
            x if x == Self::SensitivityType.code() => Ok(Self::SensitivityType),
            x if x == Self::ExifVersion.code() => Ok(Self::ExifVersion),
            x if x == Self::ApertureValue.code() => Ok(Self::ApertureValue),
            x if x == Self::BrightnessValue.code() => Ok(Self::BrightnessValue),
            x if x == Self::ExposureBiasValue.code() => Ok(Self::ExposureBiasValue),
            x if x == Self::MaxApertureValue.code() => Ok(Self::MaxApertureValue),
            x if x == Self::SubjectDistance.code() => Ok(Self::SubjectDistance),
            x if x == Self::MeteringMode.code() => Ok(Self::MeteringMode),
            x if x == Self::LightSource.code() => Ok(Self::LightSource),
            x if x == Self::Flash.code() => Ok(Self::Flash),
            x if x == Self::FocalLength.code() => Ok(Self::FocalLength),
            x if x == Self::SubjectArea.code() => Ok(Self::SubjectArea),
            x if x == Self::MakerNote.code() => Ok(Self::MakerNote),
            x if x == Self::UserComment.code() => Ok(Self::UserComment),
            x if x == Self::FlashPixVersion.code() => Ok(Self::FlashPixVersion),
            x if x == Self::ColorSpace.code() => Ok(Self::ColorSpace),
            x if x == Self::RelatedSoundFile.code() => Ok(Self::RelatedSoundFile),
            x if x == Self::FlashEnergy.code() => Ok(Self::FlashEnergy),
            x if x == Self::FocalPlaneXResolution.code() => Ok(Self::FocalPlaneXResolution),
            x if x == Self::FocalPlaneYResolution.code() => Ok(Self::FocalPlaneYResolution),
            x if x == Self::FocalPlaneResolutionUnit.code() => Ok(Self::FocalPlaneResolutionUnit),
            x if x == Self::SubjectLocation.code() => Ok(Self::SubjectLocation),
            x if x == Self::ExposureIndex.code() => Ok(Self::ExposureIndex),
            x if x == Self::SensingMethod.code() => Ok(Self::SensingMethod),
            x if x == Self::FileSource.code() => Ok(Self::FileSource),
            x if x == Self::SceneType.code() => Ok(Self::SceneType),
            x if x == Self::CFAPattern.code() => Ok(Self::CFAPattern),
            x if x == Self::CustomRendered.code() => Ok(Self::CustomRendered),
            x if x == Self::ExposureMode.code() => Ok(Self::ExposureMode),
            x if x == Self::WhiteBalanceMode.code() => Ok(Self::WhiteBalanceMode),
            x if x == Self::DigitalZoomRatio.code() => Ok(Self::DigitalZoomRatio),
            x if x == Self::FocalLengthIn35mmFilm.code() => Ok(Self::FocalLengthIn35mmFilm),
            x if x == Self::SceneCaptureType.code() => Ok(Self::SceneCaptureType),
            x if x == Self::GainControl.code() => Ok(Self::GainControl),
            x if x == Self::Contrast.code() => Ok(Self::Contrast),
            x if x == Self::Saturation.code() => Ok(Self::Saturation),
            x if x == Self::Sharpness.code() => Ok(Self::Sharpness),
            x if x == Self::DeviceSettingDescription.code() => Ok(Self::DeviceSettingDescription),
            x if x == Self::SubjectDistanceRange.code() => Ok(Self::SubjectDistanceRange),
            x if x == Self::ImageUniqueID.code() => Ok(Self::ImageUniqueID),
            x if x == Self::LensSpecification.code() => Ok(Self::LensSpecification),
            x if x == Self::LensMake.code() => Ok(Self::LensMake),
            x if x == Self::LensModel.code() => Ok(Self::LensModel),
            x if x == Self::Gamma.code() => Ok(Self::Gamma),
            x if x == Self::GPSTimeStamp.code() => Ok(Self::GPSTimeStamp),
            x if x == Self::GPSSatellites.code() => Ok(Self::GPSSatellites),
            x if x == Self::GPSStatus.code() => Ok(Self::GPSStatus),
            x if x == Self::GPSMeasureMode.code() => Ok(Self::GPSMeasureMode),
            x if x == Self::GPSDOP.code() => Ok(Self::GPSDOP),
            x if x == Self::GPSSpeedRef.code() => Ok(Self::GPSSpeedRef),
            x if x == Self::GPSSpeed.code() => Ok(Self::GPSSpeed),
            x if x == Self::GPSTrackRef.code() => Ok(Self::GPSTrackRef),
            x if x == Self::GPSTrack.code() => Ok(Self::GPSTrack),
            x if x == Self::GPSImgDirectionRef.code() => Ok(Self::GPSImgDirectionRef),
            x if x == Self::GPSImgDirection.code() => Ok(Self::GPSImgDirection),
            x if x == Self::GPSMapDatum.code() => Ok(Self::GPSMapDatum),
            x if x == Self::GPSDestLatitudeRef.code() => Ok(Self::GPSDestLatitudeRef),
            x if x == Self::GPSDestLatitude.code() => Ok(Self::GPSDestLatitude),
            x if x == Self::GPSDestLongitudeRef.code() => Ok(Self::GPSDestLongitudeRef),
            x if x == Self::GPSDestLongitude.code() => Ok(Self::GPSDestLongitude),
            x if x == Self::GPSDestBearingRef.code() => Ok(Self::GPSDestBearingRef),
            x if x == Self::GPSDestBearing.code() => Ok(Self::GPSDestBearing),
            x if x == Self::GPSDestDistanceRef.code() => Ok(Self::GPSDestDistanceRef),
            x if x == Self::GPSDestDistance.code() => Ok(Self::GPSDestDistance),
            x if x == Self::GPSProcessingMethod.code() => Ok(Self::GPSProcessingMethod),
            x if x == Self::GPSAreaInformation.code() => Ok(Self::GPSAreaInformation),
            x if x == Self::GPSDateStamp.code() => Ok(Self::GPSDateStamp),
            x if x == Self::GPSDifferential.code() => Ok(Self::GPSDifferential),
            v => Err(format!("Unrecognized Self 0x{v:04x}").into()),
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

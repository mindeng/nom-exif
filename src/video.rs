#[cfg(feature = "json_dump")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "json_dump", derive(Serialize, Deserialize))]
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
enum VideoTag {
    Duration,
    DateTimeOriginal,
    ImageWidth,
    ImageHeight,
    GpsIso6709,
}

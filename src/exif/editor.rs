use std::io::{self, Cursor, Read, Write};

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime};

use crate::{EntryValue, Exif, ExifIter, ExifTag};

use super::exif_exif::{self, IFD_ENTRY_SIZE};

pub(crate) struct TiffEditor {
    exif_iter: ExifIter,
    date_time_original: Option<NaiveDateTime>,
    time_zone: Option<FixedOffset>,
}

impl TiffEditor {
    fn new(exif_iter: ExifIter) -> Self {
        Self {
            exif_iter,
            date_time_original: None,
            time_zone: None,
        }
    }

    pub fn write_to<W: Write>(&self, mut writer: W) -> io::Result<u64> {
        let mut c = Cursor::new(&self.exif_iter.input[..]);
        let exif: Exif = self.exif_iter.clone().into();
        if self.date_time_original.is_some() && exif.get(ExifTag::DateTimeOriginal).is_none() {
            // insert
            // let mut r = c.by_ref().take(self.first_entry_data_offset());
            // io::copy(r, &mut writer)?;
        }
        Ok(0)
    }

    /// This method won't change the time zone information stored in image.
    pub fn set_date_time_original(&mut self, time: NaiveDateTime) {
        self.date_time_original = Some(time);
    }

    /// This method will change the time zone information stored in image.
    ///
    /// All the following tags expressing time zone and time will be modified
    /// accordingly (if they exist).
    ///
    /// - `OffsetTimeOriginal` : corresponds to `DateTimeOriginal`
    /// - `OffsetTimeDigitized`: corresponds to `CreateDate`
    /// - `OffsetTime`: corresponds to `ModifyDate`
    pub fn set_time_zone(&mut self, offset: FixedOffset) {}

    /// The position immediately following the Next IFD node of ifd0
    fn first_entry_data_offset(&self) -> u32 {
        self.exif_iter.ifd0.offset
            + 2                 // entry num
            + self.exif_iter.ifd0.entry_num as u32 * IFD_ENTRY_SIZE as u32 // entires
            + 4 // Next IFD
    }
}

#[cfg(test)]
mod tests {
    use test_case::case;

    use crate::{ExifIter, MediaParser, MediaSource};

    use super::TiffEditor;

    #[case("testdata/tif.tif", 0x4ba16)]
    fn first_entry_data_offset(path: &str, pos: u32) {
        let mut parser = MediaParser::new();
        let ms = MediaSource::file_path(path).unwrap();
        let iter: ExifIter = parser.parse(ms).unwrap();
        let editor = TiffEditor::new(iter);

        assert_eq!(editor.first_entry_data_offset(), pos);
    }
}

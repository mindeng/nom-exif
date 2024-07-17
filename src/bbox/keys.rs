use nom::bytes::complete::take;
use nom::combinator::{flat_map, map_res};
use nom::multi::many_m_n;
use nom::number::complete::be_u32;

use crate::bbox::{FullBoxHeader, ParseBody};

/// Represents a [keys atom][1].
///
/// `keys` is a fullbox which contains version & flags.
///
/// atom-path: moov/meta/keys
///
/// [1]: https://developer.apple.com/documentation/quicktime-file-format/metadata_item_keys_atom
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeysBox {
    header: FullBoxHeader,
    entry_count: u32,
    pub entries: Vec<KeyEntry>,
}

impl ParseBody<KeysBox> for KeysBox {
    fn parse_body(body: &[u8], header: FullBoxHeader) -> nom::IResult<&[u8], KeysBox> {
        let (remain, entry_count) = be_u32(body)?;
        let (remain, entries) =
            many_m_n(entry_count as usize, entry_count as usize, KeyEntry::parse)(remain)?;

        Ok((
            remain,
            KeysBox {
                header,
                entry_count,
                entries,
            },
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyEntry {
    size: u32,
    pub namespace: String, // 4 bytes
    pub key: String,       // len: size - 8
}

impl KeyEntry {
    fn parse<'a>(input: &'a [u8]) -> nom::IResult<&'a [u8], KeyEntry> {
        let (remain, s) = map_res(
            flat_map(
                map_res(be_u32, |len| {
                    len.checked_sub(4).ok_or("invalid KeyEntry header")
                }),
                take,
            ),
            |bs: &'a [u8]| String::from_utf8(bs.to_vec()),
        )(input)?;

        Ok((
            remain,
            KeyEntry {
                size: (s.len() + 4) as u32,
                namespace: s.chars().take(4).collect(),
                key: s.chars().skip(4).collect(),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        bbox::{travel_while, ParseBox},
        testkit::read_sample,
    };

    use super::*;
    use test_case::test_case;

    #[test_case("meta.mov", 4133, 0x01b9, 0xc9)]
    fn keys_box(path: &str, moov_size: u64, meta_size: u64, keys_size: u64) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let (_, moov) = travel_while(&buf, |b| b.box_type() != "moov").unwrap();
        let moov = moov.unwrap();
        let (_, meta) = travel_while(moov.body_data(), |b| b.box_type() != "meta").unwrap();
        let meta = meta.unwrap();
        let (_, keys) = travel_while(meta.body_data(), |b| b.box_type() != "keys").unwrap();
        let keys = keys.unwrap();

        assert_eq!(moov.box_size(), moov_size);
        assert_eq!(meta.box_size(), meta_size);
        assert_eq!(keys.box_size(), keys_size);

        let (rem, keys) = KeysBox::parse_box(keys.data).unwrap();
        assert!(rem.is_empty());

        assert_eq!(
            keys.entries,
            vec![
                KeyEntry {
                    size: 32,
                    namespace: "mdta".to_owned(),
                    key: "com.apple.quicktime.make".to_owned()
                },
                KeyEntry {
                    size: 33,
                    namespace: "mdta".to_owned(),
                    key: "com.apple.quicktime.model".to_owned()
                },
                KeyEntry {
                    size: 36,
                    namespace: "mdta".to_owned(),
                    key: "com.apple.quicktime.software".to_owned()
                },
                KeyEntry {
                    size: 44,
                    namespace: "mdta".to_owned(),
                    key: "com.apple.quicktime.location.ISO6709".to_owned()
                },
                KeyEntry {
                    size: 40,
                    namespace: "mdta".to_owned(),
                    key: "com.apple.quicktime.creationdate".to_owned()
                }
            ]
        );
    }

    #[test_case("embedded-in-heic.mov", 0x1790, 0x0372, 0x1ce)]
    fn heic_mov_keys(path: &str, moov_size: u64, meta_size: u64, keys_size: u64) {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let buf = read_sample(path).unwrap();
        let (_, moov) = travel_while(&buf, |b| b.box_type() != "moov").unwrap();
        let moov = moov.unwrap();
        let (_, meta) = travel_while(moov.body_data(), |b| b.box_type() != "meta").unwrap();
        let meta = meta.unwrap();
        let (_, keys) = travel_while(meta.body_data(), |b| b.box_type() != "keys").unwrap();
        let keys = keys.unwrap();

        assert_eq!(moov.box_size(), moov_size);
        assert_eq!(meta.box_size(), meta_size);
        assert_eq!(keys.box_size(), keys_size);

        let (rem, keys) = KeysBox::parse_box(keys.data).unwrap();
        assert!(rem.is_empty());

        let mut s = keys
            .entries
            .iter()
            .map(|x| format!("{x:?}"))
            .collect::<Vec<_>>()
            .join("\n");
        s.insert(0, '\n');

        assert_eq!(
            s,
            r#"
KeyEntry { size: 56, namespace: "mdta", key: "com.apple.quicktime.location.accuracy.horizontal" }
KeyEntry { size: 43, namespace: "mdta", key: "com.apple.quicktime.live-photo.auto" }
KeyEntry { size: 46, namespace: "mdta", key: "com.apple.quicktime.content.identifier" }
KeyEntry { size: 53, namespace: "mdta", key: "com.apple.quicktime.live-photo.vitality-score" }
KeyEntry { size: 63, namespace: "mdta", key: "com.apple.quicktime.live-photo.vitality-scoring-version" }
KeyEntry { size: 44, namespace: "mdta", key: "com.apple.quicktime.location.ISO6709" }
KeyEntry { size: 32, namespace: "mdta", key: "com.apple.quicktime.make" }
KeyEntry { size: 33, namespace: "mdta", key: "com.apple.quicktime.model" }
KeyEntry { size: 36, namespace: "mdta", key: "com.apple.quicktime.software" }
KeyEntry { size: 40, namespace: "mdta", key: "com.apple.quicktime.creationdate" }"#,
        );
    }
}

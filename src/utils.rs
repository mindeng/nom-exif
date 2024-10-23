use nom::{combinator::map_res, IResult};

pub(crate) fn parse_cstr(input: &[u8]) -> IResult<&[u8], String> {
    let (remain, s) = map_res(
        nom::bytes::streaming::take_till(|b| b == 0),
        |bs: &[u8]| {
            if bs.is_empty() {
                Ok("".to_owned())
            } else {
                String::from_utf8(bs.to_vec())
            }
        },
    )(input)?;

    // consumes the zero byte
    Ok((&remain[1..], s)) // Safe-slice
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::case;

    #[case(b"", None)]
    #[case(b"\0", Some(""))]
    #[case(b"h\0", Some("h"))]
    #[case(b"hello\0", Some("hello"))]
    #[case(b"hello", None)]
    fn test_check_raf(data: &[u8], expect: Option<&str>) {
        let res = parse_cstr(data);
        match expect {
            Some(s) => assert_eq!(res.unwrap().1, s),
            None => {
                res.unwrap_err();
            }
        }
    }
}

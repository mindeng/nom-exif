use crate::slice::SubsliceRange;
use crate::{error::convert_parse_error, input::Input, FileFormat};
use core::cmp;
use nom::Needed;
use std::io::Read;

/// Read exif data from `reader`, if `format` is None, then guess the file
/// format based on the read content.
#[tracing::instrument(skip(reader))]
pub(crate) fn read_exif<T: Read>(
    mut reader: T,
    format: Option<FileFormat>,
) -> crate::Result<Option<Input<'static>>> {
    const INIT_BUF_SIZE: usize = 4096;
    const MIN_GROW_SIZE: usize = 4096;
    const MAX_GROW_SIZE: usize = 1000 * 4096;

    let mut buf = Vec::with_capacity(INIT_BUF_SIZE);
    let n = reader
        .by_ref()
        .take(INIT_BUF_SIZE as u64)
        .read_to_end(buf.as_mut())?;
    if n == 0 {
        return Err("file is empty".into());
    }

    let ff = match format {
        Some(ff) => {
            ff.check(&buf)?;
            ff
        }
        None => buf
            .as_slice()
            .try_into()
            .map_err(|_| "unrecognized file format")?,
    };

    let exif_data = loop {
        let to_read = match ff.extract_exif_data(&buf[..]) {
            Ok((_, data)) => break data,
            Err(nom::Err::Incomplete(needed)) => match needed {
                Needed::Unknown => MIN_GROW_SIZE,
                Needed::Size(n) => n.get(),
            },
            Err(err) => return Err(convert_parse_error(err, "read exif failed")),
        };

        tracing::debug!(bytes = ?to_read, "to_read");
        assert!(to_read > 0);

        let to_read = cmp::max(MIN_GROW_SIZE, to_read);
        let to_read = cmp::min(MAX_GROW_SIZE, to_read);
        buf.reserve(to_read);

        let n = reader
            .by_ref()
            .take(to_read as u64)
            .read_to_end(buf.as_mut())?;
        if n == 0 {
            return Err("read exif failed; not enough bytes".into());
        }
    };

    Ok(exif_data
        .and_then(|x| buf.subslice_range(x))
        .map(|x| Input::from_vec_range(buf, x)))
}

#[cfg(feature = "async")]
use tokio::io::AsyncRead;
#[cfg(feature = "async")]
use tokio::io::AsyncReadExt;

/// Read exif data from `reader`, if `format` is None, then guess the file
/// format based on the read content.
#[cfg(feature = "async")]
#[tracing::instrument(skip(reader))]
pub(crate) async fn read_exif_async<T>(
    mut reader: T,
    format: Option<FileFormat>,
) -> crate::Result<Option<Input<'static>>>
where
    T: AsyncRead + Unpin + Send,
{
    use core::ops::Deref;
    const INIT_BUF_SIZE: usize = 4096;
    const MIN_GROW_SIZE: usize = 4096;
    const MAX_GROW_SIZE: usize = 1000 * 4096;

    let mut buf = Vec::with_capacity(INIT_BUF_SIZE);

    let n = reader.read_buf(&mut buf).await?;
    if n == 0 {
        return Err("file is empty".into());
    }

    let ff = match format {
        Some(ff) => {
            ff.check(&buf)?;
            ff
        }
        None => buf
            .deref()
            .try_into()
            .map_err(|_| "unrecognized file format")?,
    };

    let exif_data = loop {
        let to_read = match ff.extract_exif_data(&buf[..]) {
            Ok((_, data)) => break data,
            Err(nom::Err::Incomplete(needed)) => match needed {
                Needed::Unknown => MIN_GROW_SIZE,
                Needed::Size(n) => n.get(),
            },
            Err(err) => return Err(convert_parse_error(err, "read exif failed")),
        };

        tracing::debug!(bytes = ?to_read, "to_read");
        assert!(to_read > 0);

        let to_read = cmp::max(MIN_GROW_SIZE, to_read);
        let to_read = cmp::min(MAX_GROW_SIZE, to_read);
        buf.reserve(to_read);

        let n = reader.read_buf(&mut buf).await?;
        if n == 0 {
            return Err("read exif failed; not enough bytes".into());
        }
    };

    Ok(exif_data
        .and_then(|x| buf.subslice_range(x))
        .map(|x| Input::from_vec_range(buf, x)))
}

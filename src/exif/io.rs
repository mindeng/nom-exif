use crate::error::ParsingError;
use crate::ioutil::{BufLoader, Loader, ReadLoader};
use crate::slice::SubsliceRange;
use crate::{input::Input, FileFormat};
use std::io::Read;

/// Read exif data from `reader`, if `format` is None, then guess the file
/// format based on the read content.
#[tracing::instrument(skip(read))]
pub(crate) fn read_exif<T: Read>(
    read: T,
    format: Option<FileFormat>,
) -> crate::Result<Option<Input<'static>>> {
    let mut reader = ReadLoader::new(read);
    let ff = match format {
        Some(ff) => ff,
        None => reader.load_and_parse(|x| {
            x.try_into()
                .map_err(|_| ParsingError::Failed("unrecognized file format".to_string()))
        })?,
    };

    let exif_data = reader.load_and_parse(|buf| match ff.extract_exif_data(buf) {
        Ok((_, data)) => Ok(data.and_then(|x| buf.subslice_range(x))),
        Err(e) => Err(e.into()),
    })?;

    Ok(exif_data.map(|x| Input::from_vec_range(reader.into_vec(), x)))
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
    T: AsyncRead + std::marker::Unpin,
{
    use std::{cmp, ops::Deref};

    use nom::Needed;

    use crate::error::convert_parse_error;
    const INIT_BUF_SIZE: usize = 4096;
    const MIN_GROW_SIZE: usize = 4096;
    const MAX_GROW_SIZE: usize = 1000 * 4096;

    let mut buf = Vec::with_capacity(INIT_BUF_SIZE);

    let n = reader.read_buf(&mut buf).await?;
    if n == 0 {
        Err("file is empty")?;
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

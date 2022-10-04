use std::{io::{Read, Seek}, borrow::Cow};

pub fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/***
 * Handles range requests if needed.
 *
 * The file is served from the current position.
 */
pub fn handle_range<T: Read + Seek + Send + 'static>(
    request: &rouille::Request,
    max_len: Option<u64>,
    mod_time : Option<u64>,
    mut file: T,
) -> anyhow::Result<rouille::Response> {
    struct MaxRead<T> {
        left: u64,
        inner: T,
    }

    impl<T: Read> Read for MaxRead<T> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let read_len = std::cmp::min(self.left, buf.len() as u64);
            if read_len == 0 {
                return Ok(0);
            }
            let n = self.inner.read(&mut buf[..(read_len as usize)])?;
            self.left -= n as u64;
            Ok(n)
        }
    }

    let range = request
        .header("Range")
        .and_then(|s| s.trim().strip_prefix("bytes="))
        .and_then(|s| {
            let mut parts = s.splitn(2, '-');
            let offset = parts.next()?.parse::<u64>().ok()?;
            let end = parts.next()?.parse::<u64>().ok()?;

            let length = end.saturating_sub(offset) + 1;
            Some((offset, length))
        });

    // No If range header means do Range.
    let if_range_fullfilled = request
        .header("If-Range")
        .map(|v| match mod_time {
            Some(mod_time) => v.trim() == &format!("\"{}\"", mod_time),
            None => false,
        })
        .unwrap_or(true);
    
    // if etag changed, return 200 and full file.
    let range = if if_range_fullfilled {range} else {None};

    let current_pos = file.seek(std::io::SeekFrom::Current(0))?;
    let rest_len =
        (file.seek(std::io::SeekFrom::End(0))? - current_pos).min(max_len.unwrap_or(std::u64::MAX));
    let _ = file.seek(std::io::SeekFrom::Start(current_pos))?;

    let mut headers : Vec<(Cow<'static, str>, Cow<'static, str>)> = vec![
        ("Content-Type".into(), "application/octet-stream".into()),
    ];

    if let Some(mod_time) = mod_time {
        headers.push(("ETag".into(), format!("\"{}\"", mod_time).into()));
    }

    match range {
        Some((offset, length)) => {
            let length = length.min(rest_len.saturating_sub(offset));
            let _ = file.seek(std::io::SeekFrom::Start(current_pos + offset))?;
            let file = MaxRead {
                left: length,
                inner: file,
            };

            headers.push(("Content-Range".into(), format!("bytes {}-{}/{}", offset, offset + length - 1, rest_len).into()));
            Ok(rouille::Response {
                status_code: 206,
                headers,
                data: rouille::ResponseBody::from_reader_and_size(file, length as usize),
                upgrade: None,
            })
        }
        None => {
            let file = MaxRead {
                left: rest_len,
                inner: file,
            };

            headers.push(("Accept-Ranges".into(), "bytes".into()));
            Ok(rouille::Response {
                status_code: 200,
                headers,
                data: rouille::ResponseBody::from_reader_and_size(file, rest_len as usize),
                upgrade: None,
            })
        }
    }
}

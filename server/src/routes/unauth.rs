use crate::{
    meta::{MetaData, MetaStore},
    tar_hash::TarHash,
    tar_password::TarPassword,
    templates::TarFileInfo,
    util::{handle_range},
    AppState, responses::ErrorResponse,
};
use age::stream::StreamReader;
use askama::Template;
use rouille::Response;
use std::{
    fs::File,
    io::Write,
    io::{Read, Seek},
    path::PathBuf,
};

const DEFAULT_DOWNLOAD_TIMEOUT: u64 = 60;

struct UnfinishedBlockingFileReader {
    file: File,
    id: TarHash,
    meta: MetaStore,
    timeout: u64,
}

impl Read for UnfinishedBlockingFileReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        for _ in 0..self.timeout {
            match self.file.read(buf) {
                Ok(0) => {
                    let m = self.meta.get(&self.id).ok().flatten();
                    match m {
                        None => break,
                        Some(m) if m.finished => break,
                        Some(_) => {
                            std::thread::sleep(std::time::Duration::from_secs(1));
                        }
                    }
                }
                Ok(n) => {
                    return Ok(n);
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok(0)
    }
}

pub fn get_download_raw(
    state: &AppState,
    request: &rouille::Request,
    id: TarHash,
) -> anyhow::Result<Response> {
    let m = state.meta.get(&id)?.ok_or_else(ErrorResponse::not_found)?;

    let path = format!("data/{}.tar.age", &id);
    if m.finished {
        let m_time = std::fs::metadata(&path)?.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_secs();
        handle_range(request, None, Some(m_time), File::open(&path)?)
    } else {
        let file = File::open(&path)?;
        let reader = UnfinishedBlockingFileReader {
            file,
            id,
            meta: state.meta.clone(),
            timeout: DEFAULT_DOWNLOAD_TIMEOUT,
        };
        Ok(rouille::Response {
            status_code: 200,
            headers: vec![("Content-Type".into(), "application/octet-stream".into())],
            data: rouille::ResponseBody::from_reader(reader),
            upgrade: None,
        })
    }
}

pub fn get_download(
    state: &AppState,
    request: &rouille::Request,
    id: TarPassword,
) -> anyhow::Result<Response> {
    let hash = TarHash::from_tarid(&id, &state.config.general.hostname);

    let m = state.meta.get(&hash)?.ok_or_else(ErrorResponse::not_found)?;

    let offset = request
        .get_param("offset")
        .map(|v| v.parse::<u64>())
        .transpose()?;

    let length = request
        .get_param("length")
        .map(|v| v.parse::<u64>())
        .transpose()?;

    let name = request.get_param("name").map(|v| v.to_string());

    let path = PathBuf::from(&format!("data/{}.tar.age", hash));
    let m_time = std::fs::metadata(&path)?.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let file = std::fs::File::open(path)?;
    if !m.finished {
        if offset.is_some() || length.is_some() {
            return Ok(Response::text("Download not finished").with_status_code(417));
        }

        let reader = UnfinishedBlockingFileReader {
            file,
            id: hash,
            meta: state.meta.clone(),
            timeout: DEFAULT_DOWNLOAD_TIMEOUT,
        };
        let decryptor = match age::Decryptor::new(reader)? {
            age::Decryptor::Passphrase(decryptor) => decryptor,
            _ => return Ok(Response::empty_404()),
        };
        let de_reader =
            decryptor.decrypt(&age::secrecy::SecretString::from(id.to_string()), None)?;
        let data = rouille::ResponseBody::from_reader(de_reader);

        return Ok(rouille::Response {
            status_code: 200,
            headers: vec![("Content-Type".into(), "application/octet-stream".into())],
            data,
            upgrade: None,
        });
    }

    let decryptor = match age::Decryptor::new(file)? {
        age::Decryptor::Passphrase(decryptor) => decryptor,
        _ => return Ok(Response::empty_404()),
    };

    let mut de_reader =
        decryptor.decrypt(&age::secrecy::SecretString::from(id.to_string()), None)?;
    if let Some(offset) = offset {
        de_reader.seek(std::io::SeekFrom::Start(offset))?;
    }

    let res = handle_range(&request, length, Some(m_time), de_reader)?;
    let res = match name {
        Some(name) => res.with_content_disposition_attachment(&name),
        None => res,
    };

    Ok(res)
}

fn get_decrypted_reader(
    state: &AppState,
    id: &TarPassword,
) -> anyhow::Result<Result<(StreamReader<File>, MetaData), Response>> {
    let hash = TarHash::from_tarid(&id, &state.config.general.hostname);

    let m = state.meta.get(&hash)?.ok_or_else(ErrorResponse::not_found)?;

    if !m.finished {
        return Ok(Err(
            Response::text("Upload not finished yet").with_status_code(200)
        ));
    }

    let path = PathBuf::from(&format!("data/{}.tar.age", hash));
    let file = std::fs::File::open(path)?;

    let decryptor = match age::Decryptor::new(file)? {
        age::Decryptor::Passphrase(decryptor) => decryptor,
        _ => return Ok(Err(Response::empty_404())),
    };
    let de_reader = decryptor.decrypt(&age::secrecy::SecretString::from(id.to_string()), None)?;
    Ok(Ok((de_reader, m)))
}

pub fn get_tar_to_zip(
    state: &AppState,
    _request: &rouille::Request,
    id: TarPassword,
) -> anyhow::Result<Response> {
    struct FakeWriter {
        len: u64,
    }

    impl Write for FakeWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.len += buf.len() as u64;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct Tar2ZipReader {
        buffer: Vec<u8>,
        receiver: std::sync::mpsc::Receiver<Vec<u8>>,
    }

    struct Tar2ZipWriter {
        written: u64,
        sender: std::sync::mpsc::SyncSender<Vec<u8>>,
    }

    impl Read for Tar2ZipReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.buffer.is_empty() {
                match self.receiver.recv() {
                    Ok(v) => {
                        self.buffer = v;
                    }
                    Err(_) => return Ok(0),
                };
            }
            let n = std::cmp::min(buf.len(), self.buffer.len());
            buf[..n].copy_from_slice(&self.buffer[..n]);
            self.buffer.drain(..n);
            Ok(n)
        }
    }

    impl Write for Tar2ZipWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.sender
                .send(buf.to_vec())
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "send error"))?;
            self.written += buf.len() as u64;
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let (mut reader, _) = match get_decrypted_reader(state, &id) {
        Ok(Ok(reader)) => reader,
        Ok(Err(res)) => return Ok(res),
        Err(e) => return Err(e),
    };

    let (sender, receiver) = std::sync::mpsc::sync_channel(1024);

    let sender = Tar2ZipWriter { written: 0, sender };

    let receiver = Tar2ZipReader {
        buffer: Vec::new(),
        receiver,
    };

    let fake_writer = FakeWriter { len: 0 };

    let mut archive = tar::Archive::new(&mut reader);
    let mut zip = streaming_zip::Archive::new(fake_writer);
    let mut content_len = 0;

    for entry in archive.entries_with_seek()? {
        let entry = entry?;
        let path = entry.path()?.to_string_lossy().to_string();
        let mtime = entry.header().mtime().unwrap_or(0);
        content_len += entry.header().size().unwrap_or(0);

        zip.add_file(
            path.into(),
            chrono::NaiveDateTime::from_timestamp(mtime as i64, 0),
            streaming_zip::CompressionMode::Store,
            &mut std::io::empty(),
            true,
        )?;
    }
    let _ = reader.seek(std::io::SeekFrom::Start(0))?;
    let total_len = zip.finish()?.len + content_len;

    std::thread::spawn(move || {
        let mut archive = tar::Archive::new(reader);
        let mut zip = streaming_zip::Archive::new(sender);

        for entry in archive.entries_with_seek()? {
            let mut entry = entry?;
            let path = entry.path()?.to_string_lossy().to_string();
            let mtime = entry.header().mtime().unwrap_or(0);

            zip.add_file(
                path.into(),
                chrono::NaiveDateTime::from_timestamp(mtime as i64, 0),
                streaming_zip::CompressionMode::Store,
                &mut entry,
                true,
            )?;
        }

        let written = zip.finish()?.written;
        if written != total_len {
            eprintln!("ERROR: ZIP SIZE DOES NOT MATCH EXPECTED SIZE: written={written}, expected={total_len}.");
        }
        Ok(()) as anyhow::Result<()>
    });

    Ok(rouille::Response {
        status_code: 200,
        headers: vec![("Content-Type".into(), "application/zip ".into())],
        data: rouille::ResponseBody::from_reader_and_size(receiver, total_len as _),
        upgrade: None,
    }
    .with_content_disposition_attachment("archive.zip"))
}

pub fn get_ui_index(
    state: &AppState,
    _request: &rouille::Request,
    id: TarPassword,
) -> anyhow::Result<Response> {
    let (reader, meta_data) = match get_decrypted_reader(state, &id) {
        Ok(Ok(reader)) => reader,
        Ok(Err(res)) => return Ok(res),
        Err(e) => return Err(e),
    };

    let mut index = crate::templates::TarIndex {
        files: Vec::new(),
        hostname: state.config.general.hostname.clone(),
        id: id.to_string(),
        craeted_at: chrono::NaiveDateTime::from_timestamp(meta_data.created_at_unix as i64, 0),
        valid_until: chrono::NaiveDateTime::from_timestamp(meta_data.delete_at_unix as i64, 0),
    };

    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries_with_seek()? {
        let entry = entry?;
        let path = entry.path()?;
        if path.is_dir() {
            continue;
        }
        let name = &path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
            .to_string();

        let path = &path.to_string_lossy().to_string();

        let offset = entry.raw_file_position();
        let length = entry.size();

        let mtime = entry.header().mtime().unwrap_or(0);

        index.files.push(TarFileInfo {
            is_dir: path.ends_with("/"),
            path: path.clone(),
            name: name.clone(),
            offset,
            size: length,
            m_time: chrono::NaiveDateTime::from_timestamp(mtime as i64, 0),
        });
    }

    Ok(Response::html(index.render()?))
}
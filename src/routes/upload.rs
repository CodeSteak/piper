use std::{io::Read, path::{PathBuf}};

use rouille::{Response, websocket::{self, Websocket}};

use crate::{AppState, tar_id::TarId, tar_hash::TarHash, meta::MetaData, util::now_unix};

pub fn ws_upload(state : &AppState, request : &rouille::Request) -> anyhow::Result<Response> {
    if !check_token(&request)? {
        return Ok(Response::text("Invalid token").with_status_code(403));
    } 

    let (resp, websocket) = match websocket::start(request, None as Option<&'static str>) {
        Ok(a) => a,
        Err(_e) => {
            return Ok(Response::text("Expected Websocket").with_status_code(400));
        }
    };

    let id = TarId::generate();
    let id_str = id.to_string();
    let hash = TarHash::from_tarid(&id, state.hostname.as_str());

    let state = state.clone();
    std::thread::spawn(move || {
        let mut ws = websocket.recv().unwrap();

        let _ = ws.send_text(&format!("http://{}/{}/", state.hostname, id_str));

        struct WSReader<'a> {
            buffer : Vec<u8>,
            inner : &'a mut Websocket,
        }

        impl<'a> Read for WSReader<'a> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.buffer.len() == 0 {
                   match self.inner.next() {
                        Some(rouille::websocket::Message::Binary(b)) => {
                            self.buffer = b;
                        },
                        Some(_) => {
                            return Err(std::io::Error::new(std::io::ErrorKind::Other, "Unexpected message"));
                        },
                        None => return Ok(0),
                   }
                }
                let n = std::cmp::min(self.buffer.len(), buf.len());
                buf[..n].copy_from_slice(&self.buffer[..n]);
                self.buffer.drain(..n);
                return Ok(n);
            }
        }

        let _ = with_update_metadata(&hash, &state, || {
            let mut file = std::fs::File::create(path(&hash))?;
            let mut encryptor = age::Encryptor::with_user_passphrase(age::secrecy::SecretString::from(id_str.clone()))
                .wrap_output(&mut file)
                .unwrap();
    
            std::io::copy(&mut WSReader { buffer: vec![], inner: &mut ws }, &mut encryptor)?;
            encryptor.finish()?;
            Ok(())
        });

        let _ = ws.send_text("\nDone\n");
    });

    Ok(resp)
}

pub fn post_upload(state : &AppState, request : &rouille::Request) -> anyhow::Result<Response> {

    if !check_token(&request)? {
        return Ok(Response::text("Invalid token").with_status_code(403));
    } 

    let id = TarId::generate();
    let id_str = id.to_string();

    let hash = TarHash::from_tarid(&id, state.hostname.as_str());

    let mut body = request.data().ok_or(anyhow::anyhow!("No body"))?;
    with_update_metadata(&hash, state, || {
        let mut file = std::fs::File::create(path(&hash))?;
        let mut encryptor = age::Encryptor::with_user_passphrase(age::secrecy::SecretString::from(id_str.clone()))
            .wrap_output(&mut file)
            .unwrap();

        std::io::copy(&mut body, &mut encryptor)?;
        encryptor.finish()?;
        Ok(())
    })?;

    Ok(rouille::Response::text(format!("http://{}/{}/", state.hostname, id_str)))
}

pub fn post_upload_raw(state: &AppState, request: &rouille::Request, id: TarHash) -> anyhow::Result<Response> {
    if !check_token(&request)? {
        return Ok(Response::text("Invalid token").with_status_code(403));
    }

    if state.meta.get(&id)?.is_some() {
        return Ok(Response::text("Already exists").with_status_code(403));
    }

    let mut body = request.data().ok_or(anyhow::anyhow!("No body"))?;
    with_update_metadata(&id, state, || {
        let mut file = std::fs::File::create(&path(&id))?;
        std::io::copy(&mut body, &mut file)?;
        Ok(())
    })?;

    Ok(rouille::Response::text("ok"))
}


fn check_token(request : &rouille::Request) -> anyhow::Result<bool> {
    let token = request.header("Authorization").map(|token| token.strip_prefix("Bearer ").unwrap_or(token));
    Ok(token == Some("test"))
}

fn path(hash : &TarHash) -> PathBuf {
    PathBuf::from(format!("data/{}.tar.age", hash))
}

fn with_update_metadata<T, F : FnOnce() -> anyhow::Result<T>>(
    hash: &TarHash, 
    state : &AppState,
    f : F
) -> anyhow::Result<T> {
    let mut meta = MetaData {
        owner_token: "".to_string(),
        finished: false,
        created_at_unix: now_unix(),
        delete_at_unix: now_unix() + SEVEN_DAYS,
    };
    state.meta.set(&hash, &meta)?;
    
    let result = f();

    meta.finished = true;
    state.meta.set(&hash, &meta)?;

    if result.is_err() {
        let _ = std::fs::remove_file(&path(&hash));
        let _ = state.meta.delete(&hash);
    }

    result
}

const SEVEN_DAYS : u64 = 60 * 60 * 24 * 7;

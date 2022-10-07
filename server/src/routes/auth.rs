use common::{TarHash, TarPassword};
use std::io::Read;

use rouille::{
    websocket::{self, Websocket},
    Response,
};

use crate::{
    config::UserConfig, meta::MetaData, responses::ErrorResponse, util::now_unix, AppState,
};

pub fn ws_upload(state: &AppState, request: &rouille::Request) -> anyhow::Result<Response> {
    let user = check_token(request, state)?.clone();

    let (resp, websocket) = match websocket::start(request, None as Option<&'static str>) {
        Ok(a) => a,
        Err(_e) => {
            return Ok(Response::text("Expected Websocket").with_status_code(400));
        }
    };

    let id = TarPassword::generate();
    let id_str = id.to_string();
    let hash = TarHash::from_tarid(&id, &state.config.general.hostname);

    let state = state.clone();
    std::thread::spawn(move || {
        let mut ws = websocket.recv().unwrap();

        let _ = ws.send_text(&format!(
            "https://{}/{}/",
            &state.config.general.hostname, id_str
        ));

        struct WSReader<'a> {
            buffer: Vec<u8>,
            inner: &'a mut Websocket,
        }

        impl<'a> Read for WSReader<'a> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.buffer.is_empty() {
                    match self.inner.next() {
                        Some(rouille::websocket::Message::Binary(b)) => {
                            self.buffer = b;
                        }
                        Some(_) => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Unexpected message",
                            ));
                        }
                        None => return Ok(0),
                    }
                }
                let n = std::cmp::min(self.buffer.len(), buf.len());
                buf[..n].copy_from_slice(&self.buffer[..n]);
                self.buffer.drain(..n);

                Ok(n)
            }
        }

        let _ = with_update_metadata(&hash, &state, &user, || {
            let mut file = std::fs::File::create(state.meta.file_path(&hash))?;
            let mut encryptor = age::Encryptor::with_user_passphrase(
                age::secrecy::SecretString::from(id_str.clone()),
            )
            .wrap_output(&mut file)
            .unwrap();

            std::io::copy(
                &mut WSReader {
                    buffer: vec![],
                    inner: &mut ws,
                },
                &mut encryptor,
            )?;
            encryptor.finish()?;
            Ok(())
        });

        let _ = ws.send_text("\nDone\n");
    });

    Ok(resp)
}

pub fn post_upload(state: &AppState, request: &rouille::Request) -> anyhow::Result<Response> {
    let user = check_token(request, state)?;

    let id = TarPassword::generate();
    let id_str = id.to_string();

    let hash = TarHash::from_tarid(&id, &state.config.general.hostname);

    let mut body = request.data().ok_or_else(|| anyhow::anyhow!("No body"))?;
    with_update_metadata(&hash, state, user, || {
        let mut file = std::fs::File::create(state.meta.file_path(&hash))?;
        let mut encryptor =
            age::Encryptor::with_user_passphrase(age::secrecy::SecretString::from(id_str.clone()))
                .wrap_output(&mut file)
                .unwrap();

        std::io::copy(&mut body, &mut encryptor)?;
        encryptor.finish()?;
        Ok(())
    })?;

    Ok(rouille::Response::text(format!(
        "===\n\nhttps://{}/{}/\n\n===\n\ncurl 'https://{}/{}/' | tar -xkvf -\n\n===\n",
        &state.config.general.hostname, id_str, &state.config.general.hostname, id_str,
    )))
}

pub fn post_upload_raw(
    state: &AppState,
    request: &rouille::Request,
    id: TarHash,
) -> anyhow::Result<Response> {
    let user = check_token(request, state)?;

    if state.meta.get(&id)?.is_some() {
        return Ok(Response::text("Already exists").with_status_code(403));
    }

    let mut body = request.data().ok_or_else(|| anyhow::anyhow!("No body"))?;
    with_update_metadata(&id, state, user, || {
        let mut file = std::fs::File::create(state.meta.file_path(&id))?;
        std::io::copy(&mut body, &mut file)?;
        Ok(())
    })?;

    Ok(rouille::Response::text("ok"))
}

fn check_token<'a>(
    request: &rouille::Request,
    state: &'a AppState,
) -> anyhow::Result<&'a UserConfig> {
    let token = request
        .header("Authorization")
        .map(|token| token.strip_prefix("Bearer ").unwrap_or(token));
    let token = match token {
        Some(token) => token,
        None => return Err(ErrorResponse::unauthorized().into()),
    };

    state
        .config
        .users
        .iter()
        .find(|user| user.token == token)
        .ok_or_else(|| ErrorResponse::unauthorized().into())
}

fn with_update_metadata<T, F: FnOnce() -> anyhow::Result<T>>(
    hash: &TarHash,
    state: &AppState,
    user: &UserConfig,
    f: F,
) -> anyhow::Result<T> {
    let mut meta = MetaData {
        owner: user.username.clone(),
        finished: false,
        created_at_unix: now_unix(),
        delete_at_unix: now_unix() + SEVEN_DAYS,
        allow_write: false,
        allow_rewrite: false,
    };
    state.meta.set(hash, &meta)?;

    let result = f();

    meta.finished = true;
    state.meta.set(hash, &meta)?;

    if result.is_err() {
        let _ = std::fs::remove_file(state.meta.file_path(hash));
        let _ = state.meta.delete(hash);
    }

    result
}

pub fn delete_raw(
    state: &AppState,
    request: &rouille::Request,
    hash: TarHash,
) -> anyhow::Result<Response> {
    let user = check_token(request, state)?.clone();

    let m = if let Some(m) = state.meta.get(&hash)? {
        m
    } else {
        return Ok(ErrorResponse::not_found().into());
    };

    if m.owner != user.username {
        return Err(ErrorResponse::unauthorized().into());
    }

    let path = state.meta.file_path(&hash);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    state.meta.delete(&hash)?;

    Ok(Response::text("Deleted"))
}

pub fn delete(
    state: &AppState,
    request: &rouille::Request,
    id: TarPassword,
) -> anyhow::Result<Response> {
    let hash = TarHash::from_tarid(&id, &state.config.general.hostname);
    delete_raw(state, request, hash)
}

const SEVEN_DAYS: u64 = 60 * 60 * 24 * 7;

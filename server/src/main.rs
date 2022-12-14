use common::{TarHash, TarPassword};
use rouille::Response;

use crate::responses::ErrorResponse;

mod config;
mod meta;
mod responses;
mod routes;
mod templates;
mod util;

#[macro_use]
extern crate rouille;

#[derive(Clone)]
pub struct AppState {
    pub config: config::Config,
    pub meta: meta::MetaStore,
}

fn main() {
    let config_file = std::env::var("CONFIG_FILE").unwrap_or_else(|_| "config.toml".to_string());
    println!("Loading config from {}", config_file);

    let config = config::Config::load(&config_file).unwrap();

    let state = AppState {
        config: config.clone(),
        meta: meta::MetaStore::new("./data").unwrap(),
    };

    std::thread::spawn({
        let state = state.clone();
        move || {
            run_gc(state);
        }
    });

    println!("Listening on http://{}", &config.general.listen);
    rouille::start_server(&config.general.listen, move |request| {
        let is_browser = request
            .header("Accept")
            .map(|v| v.starts_with("text/html"))
            .unwrap_or(false);

        let res: anyhow::Result<Response> = router!(request,
            (POST) ["/upload"] => {
                routes::post_upload(&state, request)
            },
            (GET) ["/upload"] => {
                routes::ws_upload(&state, request)
            },
            (GET) ["/{id}/", id : TarPassword] => {
                if is_browser {
                    routes::get_ui_index(&state, request, id)
                } else {
                    routes::get_download(&state, request, id)
                }
            },
            (DELETE) ["/{id}/", id : TarPassword] => {
                routes::delete(&state, request, id)
            },
            (GET) ["/{id}/pipe", id : TarPassword] => {
                routes::get_download(&state, request, id)
            },
            (GET) ["/{id}/zip", id : TarPassword] => {
                routes::get_tar_to_zip(&state, request, id)
            },
            (GET) ["/raw/{id}/", id : TarHash] => {
                routes::get_download_raw(&state, request, id)
            },
            (POST) ["/raw/{id}/", id : TarHash] => {
                routes::post_upload_raw(&state, request, id)
            },
            (GET) ["/"] => {
                Ok(ErrorResponse::unimplemented().into())
            },
            _ => {
                let res = rouille::match_assets(request, "./static");

                if res.is_success() {
                    Ok(res)
                } else {
                    Ok(ErrorResponse::not_found().into())
                }
            }
        );

        match res {
            Ok(r) => r,
            Err(e) => match e.downcast::<ErrorResponse>() {
                Ok(res) => res.into(),
                Err(e) => {
                    println!("Error: {:?}", e);
                    rouille::Response::text("Internal Server Error").with_status_code(500)
                }
            },
        }
    });
}

fn run_gc(state: AppState) {
    fn inner_gc(state: &AppState) -> anyhow::Result<()> {
        let mut count = 0;
        let mut total = 0;
        let mut errors = 0;

        let now = util::now_unix();
        for (k, v) in state.meta.list()?.into_iter() {
            let delete = v.delete_at_unix < now;

            if delete {
                let path = state.meta.file_path(&k);

                match if path.exists() {
                    std::fs::remove_file(path)
                } else {
                    Ok(())
                }
                .map_err(anyhow::Error::from)
                .and_then(|_| state.meta.delete(&k))
                {
                    Err(e) => {
                        println!("Error deleting {}: {:?}", k, e);
                        errors += 1;
                    }
                    Ok(_) => {
                        count += 1;
                    }
                }
            }

            total += 1;
        }

        println!("== GC: {count} / {total}, {errors} Errors");
        Ok(())
    }

    std::thread::sleep(std::time::Duration::from_secs(
        state.config.general.gc_interval_s / 10,
    ));

    loop {
        std::thread::sleep(std::time::Duration::from_secs(
            state.config.general.gc_interval_s,
        ));
        println!("=== Running GC");
        match inner_gc(&state) {
            Ok(_) => {
                println!("=== Finished GC");
            }
            Err(e) => {
                println!("== Error: {:?}", e);
            }
        }
    }
}

use rouille::Response;
use tar_hash::TarHash;
use tar_id::TarId;

mod bip39;
mod tar_id;
mod tar_hash;
mod meta;
mod routes;
mod util;

#[macro_use]
extern crate rouille;

#[derive(Clone)]
pub struct AppState {
    pub hostname : String,
    pub meta : meta::MetaStore,
}


fn main() {
    let addr = "[::1]:8000";

    let state = AppState { 
        hostname: "localhost:8000".to_string(),
        meta: meta::MetaStore::new("./data"),
    };

    println!("Listening on http://{}", addr);
    rouille::start_server(addr, move |request| {
        let is_browser = request.header("Accept").map(|v| v.starts_with("text/html")).unwrap_or(false);

        let res: anyhow::Result<Response> = router!(request,
            (POST) ["/upload"] => {
                routes::post_upload(&state, request)
            },
            (GET) ["/upload"] => {
                routes::ws_upload(&state, request)
            },
            (GET) ["/{id}/", id : TarId] => {
                if is_browser {
                    routes::get_ui_index(&state, request, id)
                } else {
                    routes::get_download(&state, request, id)
                }
            },
            (GET) ["/{id}/pipe", id : TarId] => {
                routes::get_download(&state, request, id)
            },
            (GET) ["/{id}/zip", id : TarId] => {
                routes::get_tar_to_zip(&state, request, id)
            },
            (GET) ["/raw/{id}/", id : TarHash] => {
                routes::get_download_raw(&state, request, id)
            },
            (POST) ["/raw/{id}/", id : TarHash] => {
                routes::post_upload_raw(&state, request, id)
            },
            _ => Ok(rouille::Response::empty_404())
        );

        match res {
            Ok(r) => r,
            Err(e) => {
                println!("Error: {:?}", e);
                rouille::Response::text("Internal Server Error").with_status_code(500)
            }
        }
    });
}

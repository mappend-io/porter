use crate::viewer::embedded_response;
use axum::{
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "terrarium/dist/"]
struct TerrariumViewer;

pub async fn index_handler(request_headers: HeaderMap) -> impl IntoResponse {
    serve_file("index.html", &request_headers)
}

pub async fn asset_handler(
    Path(path): Path<String>,
    request_headers: HeaderMap,
) -> impl IntoResponse {
    serve_file(&format!("assets/{path}"), &request_headers)
}

fn serve_file(path: &str, request_headers: &HeaderMap) -> Response {
    match TerrariumViewer::get(path) {
        Some(file) => embedded_response(path, file, request_headers),
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

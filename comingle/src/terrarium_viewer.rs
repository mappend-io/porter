use axum::{
    body::Body,
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "terrarium/dist/"]
struct TerrariumViewer;

pub async fn index_handler() -> impl IntoResponse {
    serve_file("index.html", "text/html")
}

pub async fn asset_handler(Path(path): Path<String>) -> impl IntoResponse {
    serve_file(
        &format!("assets/{path}"),
        &mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string(),
    )
}

fn serve_file(path: &str, mime: &str) -> Response {
    match TerrariumViewer::get(path) {
        Some(content) => Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .body(Body::from(content.data.into_owned()))
            .unwrap(),
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

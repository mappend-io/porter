use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "viewer/dist/"]
struct Viewer;

/// Routes for the files at the root of the viewer, like `/favicon.svg`. The
/// fallback never sees them: the `/{id}` layer route matches first.
pub fn root_file_routes<S: Clone + Send + Sync + 'static>(
    mut router: axum::Router<S>,
) -> axum::Router<S> {
    use axum::routing::get;
    for file in Viewer::iter().filter(|file| !file.contains('/')) {
        router = router.route(&format!("/{file}"), get(static_handler));
    }
    router
}

pub async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Viewer::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

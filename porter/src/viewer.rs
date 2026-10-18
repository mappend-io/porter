use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::{EmbeddedFile, RustEmbed};
use std::borrow::Cow;

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

pub async fn static_handler(uri: Uri, request_headers: HeaderMap) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Viewer::get(path) {
        Some(file) => embedded_response(path, file, &request_headers),
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

/// Respond with a file embedded at build time. Files under `assets/` have
/// their hash in their name, so they're immutable; the rest revalidate with
/// an ETag from the file's hash. The embedded bytes are served as they are,
/// not copied per request.
pub fn embedded_response(path: &str, file: EmbeddedFile, request_headers: &HeaderMap) -> Response {
    let hash = file.metadata.sha256_hash();
    let etag = format!(
        "\"{}\"",
        hash[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, no-cache"
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).expect("hex etag is a valid header value"),
    );

    let matches = request_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .map(|tag| tag.trim().trim_start_matches("W/"))
                .any(|tag| tag == etag || tag == "*")
        });
    if matches {
        return (StatusCode::NOT_MODIFIED, headers).into_response();
    }

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref())
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    let body = match file.data {
        Cow::Borrowed(bytes) => Body::from(bytes::Bytes::from_static(bytes)),
        Cow::Owned(bytes) => Body::from(bytes),
    };
    (headers, body).into_response()
}

use axum::body::Body;
use axum::http::Request;
use axum::http::header::CACHE_CONTROL;
use axum::middleware::Next;
use axum::response::IntoResponse;

pub async fn cache_short(req: Request<Body>, next: Next) -> impl IntoResponse {
    let mut res = next.run(req).await;
    res.headers_mut()
        .insert(CACHE_CONTROL, "public, max-age=24".parse().unwrap());
    res
}

pub async fn cache_forever(req: Request<Body>, next: Next) -> impl IntoResponse {
    let mut res = next.run(req).await;
    res.headers_mut().insert(
        CACHE_CONTROL,
        "public, max-age=31536000, immutable".parse().unwrap(), // 1 year
    );
    res
}

pub async fn security_headers(req: Request<Body>, next: Next) -> impl IntoResponse {
    use axum::http::HeaderValue;

    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static(
            "default-src 'self'; \
             script-src 'self' 'unsafe-eval'; \
             worker-src blob:; \
             object-src 'none'; \
             base-uri 'self'; \
             img-src 'self' data: blob: https://*.tile.openstreetmap.org; \
             connect-src 'self' https://*.tile.openstreetmap.org; \
             style-src 'self' 'unsafe-inline'; \
             frame-ancestors 'none';",
        ),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "Referrer-Policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );

    response
}

use axum::body::Body;
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn cache_short(req: Request<Body>, next: Next) -> impl IntoResponse {
    let mut res = next.run(req).await;
    set_cache_control(&mut res, "public, max-age=24");
    res
}

pub async fn cache_forever(req: Request<Body>, next: Next) -> impl IntoResponse {
    let mut res = next.run(req).await;
    set_cache_control(&mut res, "public, max-age=31536000, immutable"); // 1 year
    res
}

// Only successful responses are cacheable for long. Missing data is a real
// answer, cached briefly so repeated requests for an area without data don't
// each go back to the source. Other errors can come from transient source
// failures, and must not stick in clients or CDNs.
fn set_cache_control(res: &mut Response, cacheable: &'static str) {
    let value = match res.status() {
        status if status.is_success() => cacheable,
        StatusCode::NOT_FOUND => "public, max-age=60",
        _ => "no-store",
    };
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(value));
}

pub async fn security_headers(req: Request<Body>, next: Next) -> impl IntoResponse {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::middleware::from_fn;
    use axum::routing::get;
    use tower::ServiceExt;

    async fn cache_control(path: &str) -> String {
        let app = Router::new()
            .route("/ok", get(|| async { "tile" }))
            .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/failed",
                get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
            )
            .route_layer(from_fn(cache_forever));
        let req = Request::builder().uri(path).body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        res.headers()[CACHE_CONTROL].to_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn only_successful_responses_are_cached_for_long() {
        assert_eq!(
            cache_control("/ok").await,
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control("/missing").await, "public, max-age=60");
        assert_eq!(cache_control("/failed").await, "no-store");
    }
}

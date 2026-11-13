//! Serves the exported UI (static files) from the same port as the API, so the browser needs no proxy and no Node.
use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, Method, StatusCode};
use axum::response::Response;
use axum::Router;
use std::path::{Component, Path, PathBuf};

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The file a URL path means: the file itself, `<path>.html` (Next's export), or `<path>/index.html`.
fn resolve(dir: &Path, url_path: &str) -> Option<PathBuf> {
    let rel = Path::new(url_path.trim_start_matches('/'));
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None; // no `..`, no drive letters
    }
    let base = dir.join(rel);
    let candidates = [base.clone(), base.with_extension("html"), base.join("index.html")];
    candidates.into_iter().find(|p| p.is_file()).or_else(|| next_page_data(dir, rel))
}

/// Next 16 exports a page's data as `<dir>/__next.<a>/<b>/__PAGE__.txt` but asks for `<dir>/__next.<a>.<b>.__PAGE__.txt`.
fn next_page_data(dir: &Path, rel: &Path) -> Option<PathBuf> {
    let name = rel.file_name()?.to_str()?;
    let parts: Vec<&str> = name.strip_prefix("__next.")?.strip_suffix(".txt")?.split('.').collect();
    let (last, head) = parts.split_last()?;
    let (first, middle) = head.split_first()?;
    let mut p = dir.join(rel.parent()?).join(format!("__next.{first}"));
    p.extend(middle);
    p.push(format!("{last}.txt"));
    p.is_file().then_some(p)
}

fn reply(status: StatusCode, ct: &str, body: Vec<u8>) -> Response {
    Response::builder().status(status).header(header::CONTENT_TYPE, ct).body(Body::from(body)).unwrap_or_default()
}

/// Adds the UI as the fallback for every path the API does not own. `/api/*` that matches no route stays a JSON 404.
pub fn with_ui(router: Router, dir: PathBuf) -> Router {
    router.fallback(move |req: Request| {
        let dir = dir.clone();
        async move {
            let path = req.uri().path().to_string();
            if path.starts_with("/api/") || !matches!(*req.method(), Method::GET | Method::HEAD) {
                return reply(StatusCode::NOT_FOUND, "application/json", br#"{"detail":"Not Found"}"#.to_vec());
            }
            match resolve(&dir, &path).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
                Some((p, bytes)) => reply(StatusCode::OK, content_type(&p), bytes),
                None => match std::fs::read(dir.join("404.html")) {
                    Ok(b) => reply(StatusCode::NOT_FOUND, "text/html; charset=utf-8", b),
                    Err(_) => reply(StatusCode::NOT_FOUND, "text/plain; charset=utf-8", b"not found".to_vec()),
                },
            }
        }
    })
}

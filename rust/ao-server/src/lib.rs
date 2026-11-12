//! The HTTP layer: the same routes, JSON, status codes and error text as the Python backend (`backend/app/main.py`).
//!
//! Handlers are plain synchronous functions `fn(&AppState, &Req) -> R`. The framework below mounts each one on axum,
//! checks the session (cookie or bearer token) for guarded routes, parses the JSON body, runs the handler on a blocking
//! thread (the core is synchronous) and turns the result into `{"detail": ...}` errors like FastAPI did.
pub mod routes;
pub mod runs;

use ao_core::providers::base::{Provider, Transport};
use ao_core::providers::BuildError;
use axum::body::{Body, Bytes};
use axum::extract::{Query, RawPathParams, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::Response;
use axum::routing::{on, MethodFilter};
use axum::Router;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// Builds a provider from a stored record. Tests replace it; the app uses `ao_core::providers::build_with`.
pub type ProviderFactory = Arc<dyn Fn(&Value) -> Result<Box<dyn Provider>, BuildError> + Send + Sync>;

#[derive(Clone, Default)]
pub struct AppState {
    /// The HTTP transport providers use (tests inject one; None means the real network).
    pub http: Option<Transport>,
    pub provider_factory: Option<ProviderFactory>,
}

impl AppState {
    pub fn build_provider(&self, record: &Value) -> Result<Box<dyn Provider>, BuildError> {
        match &self.provider_factory {
            Some(f) => f(record),
            None => ao_core::providers::build_with(record, self.http.clone()),
        }
    }
}

pub struct Req {
    pub params: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub body: Value,
    pub headers: HeaderMap,
}

impl Req {
    pub fn param(&self, k: &str) -> &str {
        self.params.get(k).map_or("", |s| s.as_str())
    }

    /// A required string field of the JSON body (422 when missing, like a pydantic model).
    pub fn field(&self, k: &str) -> Result<String, ApiError> {
        match self.body.get(k).and_then(|v| v.as_str()) {
            Some(s) => Ok(s.to_string()),
            None => Err(ApiError::new(422, format!("field required: {k}"))),
        }
    }

    pub fn opt_str(&self, k: &str) -> Option<String> {
        self.body.get(k).and_then(|v| v.as_str()).map(String::from)
    }
}

#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub detail: Value,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Value::String(s) => f.write_str(s),
            other => write!(f, "{other}"),
        }
    }
}

impl ApiError {
    pub fn new(status: u16, detail: impl Into<String>) -> Self {
        ApiError { status, detail: Value::String(detail.into()) }
    }
}

macro_rules! from_err {
    ($($t:ty),*) => {$(impl From<$t> for ApiError {
        fn from(e: $t) -> Self {
            ApiError::new(500, e.to_string())
        }
    })*};
}
from_err!(ao_core::db::DbError, std::io::Error);

pub struct Reply {
    pub status: u16,
    pub body: Value,
    pub cookies: Vec<String>,
    pub raw: Option<(String, Vec<u8>)>,
}

pub type R = Result<Reply, ApiError>;

pub fn ok(body: Value) -> R {
    Ok(Reply { status: 200, body, cookies: vec![], raw: None })
}

pub fn bad(status: u16, detail: impl Into<String>) -> R {
    Err(ApiError::new(status, detail))
}

type Handler = fn(&AppState, &Req) -> R;

pub const SESSION_COOKIE: &str = "ao_session";

fn cookie_value(h: &HeaderMap, name: &str) -> Option<String> {
    let raw = h.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').filter_map(|p| p.trim().split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
}

fn authorised(h: &HeaderMap) -> bool {
    let mut token = cookie_value(h, SESSION_COOKIE);
    if let Some(a) = h.get(header::AUTHORIZATION).and_then(|a| a.to_str().ok()) {
        if a.to_lowercase().starts_with("bearer ") {
            token = Some(a[7..].to_string());
        }
    }
    ao_core::auth::valid_token(token.as_deref())
}

fn dispatch(st: AppState, h: Handler, guard: bool, params: HashMap<String, String>, query: HashMap<String, String>, headers: HeaderMap, body: Bytes) -> Response {
    let result = (|| {
        if guard && !authorised(&headers) {
            return Err(ApiError::new(401, "not authenticated"));
        }
        let body = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).map_err(|e| ApiError::new(422, format!("invalid JSON body: {e}")))?
        };
        h(&st, &Req { params, query, body, headers })
    })();
    match result {
        Ok(r) => {
            let mut b = Response::builder().status(r.status);
            for c in &r.cookies {
                b = b.header(header::SET_COOKIE, c);
            }
            match r.raw {
                Some((ct, bytes)) => b.header(header::CONTENT_TYPE, ct).body(Body::from(bytes)),
                None => b.header(header::CONTENT_TYPE, "application/json").body(Body::from(r.body.to_string())),
            }
            .unwrap_or_default()
        }
        Err(e) => Response::builder()
            .status(StatusCode::from_u16(e.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"detail": e.detail}).to_string()))
            .unwrap_or_default(),
    }
}

/// Collects routes, then becomes an axum `Router`.
pub struct Routes {
    router: Router<AppState>,
}

impl Routes {
    pub fn new() -> Self {
        Routes { router: Router::new() }
    }

    fn mount(mut self, method: Method, path: &str, guard: bool, h: Handler) -> Self {
        let filter = match method {
            Method::GET => MethodFilter::GET,
            Method::POST => MethodFilter::POST,
            Method::PUT => MethodFilter::PUT,
            Method::DELETE => MethodFilter::DELETE,
            _ => MethodFilter::PATCH,
        };
        let f = move |State(st): State<AppState>, raw: RawPathParams, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, body: Bytes| async move {
            let params: HashMap<String, String> = raw.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            tokio::task::spawn_blocking(move || dispatch(st, h, guard, params, q, headers, body)).await.unwrap_or_else(|_| {
                Response::builder().status(500).body(Body::from(json!({"detail": "internal error"}).to_string())).unwrap_or_default()
            })
        };
        self.router = self.router.route(path, on(filter, f));
        self
    }

    /// A route that needs a logged-in session.
    pub fn guarded(self, method: Method, path: &str, h: Handler) -> Self {
        self.mount(method, path, true, h)
    }

    /// A route open to everyone (health, auth, the registry's own endpoints).
    pub fn open(self, method: Method, path: &str, h: Handler) -> Self {
        self.mount(method, path, false, h)
    }

    pub fn into_router(self, st: AppState) -> Router {
        self.router.with_state(st)
    }
}

impl Default for Routes {
    fn default() -> Self {
        Self::new()
    }
}

pub fn app(st: AppState) -> Router {
    let _ = ao_core::db::init_db();
    routes::all().into_router(st)
}

//! Test client: calls the router in process (no sockets), keeps the session cookie like a browser, and gives each
//! test a fresh data folder.
#![allow(dead_code)]
use ao_core::providers::base::{HttpRequest, HttpResponse, Transport};
use ao_server::{app, AppState};
use axum::body::{to_bytes, Body};
use axum::http::{header, Request};
use axum::Router;
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard};
use tower::ServiceExt;

static ENV: Mutex<()> = Mutex::new(());

type Handler = Arc<dyn Fn(&HttpRequest) -> HttpResponse + Send + Sync>;

/// The fake network behind the providers: a test sets what the upstream answers.
#[derive(Clone)]
pub struct Upstream(Arc<Mutex<Handler>>);

impl Upstream {
    pub fn new() -> Self {
        Upstream(Arc::new(Mutex::new(Arc::new(|_| HttpResponse { status: 500, body: String::new() }))))
    }
    pub fn set(&self, f: impl Fn(&HttpRequest) -> HttpResponse + Send + Sync + 'static) {
        *self.0.lock().unwrap() = Arc::new(f);
    }
    pub fn transport(&self) -> Transport {
        let me = self.clone();
        Arc::new(move |req| {
            let h = me.0.lock().unwrap().clone();
            Ok(h(req))
        })
    }
}

pub struct Client {
    router: Router,
    cookie: Mutex<Option<String>>,
    pub upstream: Upstream,
}

pub struct Env {
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    pub c: Client,
}

/// A fresh data folder and a client for it. Hold the returned value for the whole test.
pub fn env() -> Env {
    env_with(|_| {})
}

pub fn env_with(tweak: impl FnOnce(&mut AppState)) -> Env {
    let guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("AO_DATA_DIR", dir.path());
    let upstream = Upstream::new();
    let mut st = AppState { http: Some(upstream.transport()), ..Default::default() };
    tweak(&mut st);
    Env { _guard: guard, _dir: dir, c: Client { router: app(st), cookie: Mutex::new(None), upstream } }
}

pub fn ok_json(status: u16, body: Value) -> HttpResponse {
    HttpResponse { status, body: body.to_string() }
}

impl Client {
    pub async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.call_with(method, path, body, &[]).await
    }

    /// Like `call`, with extra request headers.
    pub async fn call_with(&self, method: &str, path: &str, body: Option<Value>, headers: &[(&str, &str)]) -> (u16, Value) {
        let mut b = Request::builder().method(method).uri(path);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        if let Some(c) = self.cookie.lock().unwrap().as_ref() {
            b = b.header(header::COOKIE, format!("ao_session={c}"));
        }
        let req = match body {
            Some(v) => b.header(header::CONTENT_TYPE, "application/json").body(Body::from(v.to_string())),
            None => b.body(Body::empty()),
        }
        .unwrap();
        let resp = self.router.clone().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        for sc in resp.headers().get_all(header::SET_COOKIE) {
            let s = sc.to_str().unwrap();
            if let Some(rest) = s.strip_prefix("ao_session=") {
                let value = rest.split(';').next().unwrap_or("").trim_matches('"').to_string();
                *self.cookie.lock().unwrap() = if s.contains("Max-Age=0") || value.is_empty() { None } else { Some(value) };
            }
        }
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let v = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, v)
    }

    pub async fn get(&self, p: &str) -> (u16, Value) {
        self.call("GET", p, None).await
    }
    pub async fn post(&self, p: &str, b: Value) -> (u16, Value) {
        self.call("POST", p, Some(b)).await
    }
    pub async fn put(&self, p: &str, b: Value) -> (u16, Value) {
        self.call("PUT", p, Some(b)).await
    }
    pub async fn delete(&self, p: &str) -> (u16, Value) {
        self.call("DELETE", p, None).await
    }
    /// A POST with no body (a plain action).
    pub async fn act(&self, p: &str) -> (u16, Value) {
        self.call("POST", p, None).await
    }

    pub fn clear_cookies(&self) {
        *self.cookie.lock().unwrap() = None;
    }
    pub fn set_cookie(&self, v: &str) {
        *self.cookie.lock().unwrap() = Some(v.to_string());
    }

    /// Sets the password (which logs in) and returns self, like the `authed` fixture.
    pub async fn authed(&self) -> &Self {
        assert_eq!(self.post("/api/auth/setup", serde_json::json!({"password": "correct-horse-1"})).await.0, 200);
        self
    }
}

//! The REST client. The bearer token lives in `localStorage` (keep me signed in) or `sessionStorage`.

use std::cell::RefCell;

use gloo_net::http::Request;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracer_api::*;

const KEY: &str = "tracer:token";

thread_local! {
    static TOKEN: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl ApiError {
    pub fn unauthorized(&self) -> bool {
        self.status == 401
    }
}

fn storage(local: bool) -> Option<web_sys::Storage> {
    let w = web_sys::window()?;
    if local { w.local_storage().ok().flatten() } else { w.session_storage().ok().flatten() }
}

pub fn load_token() -> Option<String> {
    let t = storage(true).and_then(|s| s.get_item(KEY).ok().flatten()).or_else(|| storage(false).and_then(|s| s.get_item(KEY).ok().flatten()));
    TOKEN.with(|c| *c.borrow_mut() = t.clone());
    t
}

pub fn save_token(token: &str, keep: bool) {
    clear_token();
    if let Some(s) = storage(keep) {
        let _ = s.set_item(KEY, token);
    }
    TOKEN.with(|c| *c.borrow_mut() = Some(token.to_string()));
}

pub fn clear_token() {
    for local in [true, false] {
        if let Some(s) = storage(local) {
            let _ = s.remove_item(KEY);
        }
    }
    TOKEN.with(|c| *c.borrow_mut() = None);
}

pub fn token() -> Option<String> {
    TOKEN.with(|c| c.borrow().clone())
}

async fn finish<T: DeserializeOwned>(res: gloo_net::http::Response) -> Result<T, ApiError> {
    let status = res.status();
    if res.ok() {
        return res.json::<T>().await.map_err(|e| ApiError { status, message: format!("unexpected reply: {e}") });
    }
    let message = res
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| format!("request failed ({status})"));
    Err(ApiError { status, message })
}

fn net(e: gloo_net::Error) -> ApiError {
    ApiError { status: 0, message: format!("cannot reach the server: {e}") }
}

fn auth(req: gloo_net::http::RequestBuilder) -> gloo_net::http::RequestBuilder {
    match token() {
        Some(t) => req.header("Authorization", &format!("Bearer {t}")),
        None => req,
    }
}

pub async fn get<T: DeserializeOwned>(path: &str) -> Result<T, ApiError> {
    finish(auth(Request::get(&format!("/api{path}"))).send().await.map_err(net)?).await
}

pub async fn send<T: DeserializeOwned>(method: &str, path: &str, body: &impl Serialize) -> Result<T, ApiError> {
    let url = format!("/api{path}");
    let req = match method {
        "PATCH" => Request::patch(&url),
        "DELETE" => Request::delete(&url),
        _ => Request::post(&url),
    };
    finish(auth(req).json(body).map_err(net)?.send().await.map_err(net)?).await
}

pub async fn post<T: DeserializeOwned>(path: &str, body: &impl Serialize) -> Result<T, ApiError> {
    send("POST", path, body).await
}

pub async fn patch<T: DeserializeOwned>(path: &str, body: &impl Serialize) -> Result<T, ApiError> {
    send("PATCH", path, body).await
}

pub async fn delete(path: &str) -> Result<serde_json::Value, ApiError> {
    finish(auth(Request::delete(&format!("/api{path}"))).send().await.map_err(net)?).await
}

/// A file as the raw request body.
pub async fn upload(path: &str, mime: &str, bytes: js_sys::Uint8Array) -> Result<Attachment, ApiError> {
    let req = auth(Request::post(&format!("/api{path}"))).header("Content-Type", mime).body(bytes).map_err(net)?;
    finish(req.send().await.map_err(net)?).await
}

/// What the browser may show in place. Everything else is only ever saved to disk: the server sends
/// the same list, and an opened blob runs as part of this app, so a page or script must never be shown.
const SHOWN_IN_PLACE: [&str; 5] = ["image/png", "image/jpeg", "image/gif", "image/webp", "application/pdf"];

/// Fetch an attachment with the token: pictures and pdfs open in a new tab, anything else is saved.
pub async fn open_attachment(id: i64, name: &str) -> Result<(), ApiError> {
    use wasm_bindgen::JsCast;
    let fail = || ApiError { status: 0, message: "could not open the file".into() };
    let res = auth(Request::get(&format!("/api/attachments/{id}"))).send().await.map_err(net)?;
    if !res.ok() {
        return Err(ApiError { status: res.status(), message: "could not open the file".into() });
    }
    let bytes = res.binary().await.map_err(net)?;
    let mime = res.headers().get("content-type").unwrap_or_default().split(';').next().unwrap_or("").trim().to_lowercase();
    let show = SHOWN_IN_PLACE.contains(&mime.as_str());
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes.as_slice()));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(if show { &mime } else { "application/octet-stream" });
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(|_| fail())?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| fail())?;
    if show {
        if let Some(w) = web_sys::window() {
            let _ = w.open_with_url_and_target(&url, "_blank");
        }
    } else {
        let doc = web_sys::window().and_then(|w| w.document()).ok_or_else(fail)?;
        let a: web_sys::HtmlAnchorElement = doc.create_element("a").map_err(|_| fail())?.dyn_into().map_err(|_| fail())?;
        a.set_href(&url);
        a.set_download(name);
        a.click();
        let _ = web_sys::Url::revoke_object_url(&url);
    }
    Ok(())
}

// ---- typed calls ---------------------------------------------------------------------------------------

/// Which providers people can sign in with here.
pub async fn providers() -> Vec<String> {
    get("/auth/providers").await.unwrap_or_default()
}

/// What follows the `#` in the address, decoded as `a=b&c=d`.
pub fn hash_params() -> std::collections::HashMap<String, String> {
    let hash = web_sys::window().and_then(|w| w.location().hash().ok()).unwrap_or_default();
    serde_urlencoded::from_str(hash.trim_start_matches('#')).unwrap_or_default()
}

pub async fn me() -> Result<Me, ApiError> {
    get("/me").await
}

pub async fn accounts() -> Result<Vec<Account>, ApiError> {
    get("/accounts").await
}

pub async fn tags() -> Result<Vec<String>, ApiError> {
    #[derive(serde::Deserialize)]
    struct T {
        tag: String,
    }
    Ok(get::<Vec<T>>("/tags").await?.into_iter().map(|t| t.tag).collect())
}

pub async fn subscriptions() -> Result<Vec<Subscription>, ApiError> {
    get("/subscriptions").await
}

pub async fn assets() -> Result<Vec<Asset>, ApiError> {
    get("/assets").await
}

pub async fn notifications() -> Result<Vec<Notification>, ApiError> {
    get("/notifications").await
}

pub async fn transactions(f: &TxFilter) -> Result<TxPage, ApiError> {
    let q = serde_urlencoded::to_string(f).unwrap_or_default();
    get(&format!("/transactions?{q}")).await
}

pub async fn insights(member: Option<i64>) -> Result<Insights, ApiError> {
    let q = serde_urlencoded::to_string(InsightsQuery { member_id: member, days: None }).unwrap_or_default();
    get(&format!("/insights?{q}")).await
}

/// Fetch a file with the token and hand it to the browser to save.
pub async fn download(path: &str, filename: &str) -> Result<(), ApiError> {
    use wasm_bindgen::JsCast;
    let res = auth(Request::get(&format!("/api{path}"))).send().await.map_err(net)?;
    if !res.ok() {
        return Err(ApiError { status: res.status(), message: "could not export".into() });
    }
    let text = res.text().await.map_err(net)?;
    let parts = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(&text));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("text/csv");
    let fail = || ApiError { status: 0, message: "could not export".into() };
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts).map_err(|_| fail())?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| fail())?;
    let doc = web_sys::window().and_then(|w| w.document()).ok_or_else(fail)?;
    let a: web_sys::HtmlAnchorElement = doc.create_element("a").map_err(|_| fail())?.dyn_into().map_err(|_| fail())?;
    a.set_href(&url);
    a.set_download(filename);
    a.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}

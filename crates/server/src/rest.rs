use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Extension, FromRequestParts, Path, Query, State};
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_DISPOSITION, COOKIE, LOCATION, SET_COOKIE, CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::compression::CompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tower_http::services::{ServeDir, ServeFile};
use crate::mail::Mailer;
use pebblelab_core::api::*;
use pebblelab_core::{Caller, Error, Store};

use crate::limit::Limits;
use crate::mcp;
use crate::oauth::Oauth;

pub struct ApiError(Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::Forbidden(_) => StatusCode::FORBIDDEN,
            Error::NotFound(_) => StatusCode::NOT_FOUND,
            Error::BadRequest(_) => StatusCode::BAD_REQUEST,
            Error::Conflict(_) => StatusCode::CONFLICT,
            Error::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::Limited(_) => StatusCode::TOO_MANY_REQUESTS,
        };
        (status, Json(json!({ "error": self.0.message() }))).into_response()
    }
}

type R<T> = Result<Json<T>, ApiError>;

/// The signed-in caller from `Authorization: Bearer <token>`. Keeps the raw token for sign-out.
pub struct Auth(pub Caller, pub String);

impl FromRequestParts<Store> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, store: &Store) -> Result<Self, ApiError> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .ok_or(Error::Unauthorized)?;
        Ok(Auth(store.authenticate(token).await?, token.to_string()))
    }
}

/// Where a request came from: the socket, or the last address a proxy we trust put in `X-Forwarded-For`.
pub struct ClientIp(pub String);

impl FromRequestParts<Store> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &Store) -> Result<Self, ApiError> {
        let trust = parts.extensions.get::<Arc<Limits>>().is_some_and(|l| l.trust_proxy);
        let forwarded = trust
            .then(|| parts.headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.rsplit(',').next()).map(|v| v.trim().to_string()))
            .flatten()
            .filter(|v| !v.is_empty());
        let ip = forwarded.or_else(|| parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string())).unwrap_or_default();
        Ok(ClientIp(ip))
    }
}

/// What the browser may do with the app: its own scripts, styles, fonts and requests, and nothing framed.
const APP_CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
    img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; object-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// How long a request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub fn router(store: Store, ui_dir: PathBuf, limits: Arc<Limits>, oauth: Arc<Oauth>, mailer: Arc<Mailer>, public_url: String) -> Router {
    let api = Router::new()
        .route("/auth/signup", post(sign_up))
        .route("/auth/signin", post(sign_in))
        .route("/auth/signout", post(sign_out))
        .route("/auth/signout-all", post(sign_out_all))
        .route("/auth/reset", post(reset))
        .route("/auth/reset/confirm", post(reset_confirm))
        .route("/auth/providers", get(providers))
        .route("/auth/options", get(options))
        .route("/auth/redeem", post(redeem))
        .route("/auth/{provider}/start", get(oauth_start))
        .route("/auth/{provider}/callback", get(oauth_callback))
        .route("/me", get(me).patch(update_me).delete(delete_me))
        .route("/export.csv", get(export_csv))
        .route("/me/password", post(password))
        .route("/family", post(create_family).delete(delete_family))
        .route("/family/invite", post(invite))
        .route("/family/leave", post(leave_family))
        .route("/family/join", post(join_family))
        .route("/accounts", get(accounts).post(create_account))
        .route("/accounts/{id}", get(account).patch(update_account).delete(delete_account))
        .route("/accounts/{id}/leave", post(leave_account))
        .route("/transactions", get(transactions).post(add_transaction))
        .route("/transactions/{id}", get(transaction).patch(update_transaction).delete(delete_transaction))
        .route("/transactions/{id}/attachments", post(add_attachment))
        .route("/attachments/{id}", get(attachment).delete(delete_attachment))
        .route("/transfers", post(transfer))
        .route("/subscriptions", get(subscriptions).post(add_subscription))
        .route("/subscriptions/{id}", axum::routing::patch(update_subscription).delete(delete_subscription))
        .route("/assets", get(assets).post(add_asset))
        .route("/assets/{id}", axum::routing::patch(update_asset).delete(delete_asset))
        .route("/tags", get(tags))
        .route("/insights", get(insights))
        .route("/ask", post(ask))
        .route("/notifications", get(notifications))
        .route("/notifications/read", post(read_notifications))
        .route("/connectors", get(connectors).post(create_connector))
        .route("/connectors/{id}", delete(revoke_connector).patch(update_connector))
        .fallback(|| async { (StatusCode::NOT_FOUND, Json(json!({ "error": "no such route" }))) })
        .layer(DefaultBodyLimit::max(12 * 1024 * 1024));
    let header = |name: HeaderName, value: &'static str| SetResponseHeaderLayer::if_not_present(name, HeaderValue::from_static(value));
    Router::new()
        .route("/health", get(health))
        .route("/mcp", post(mcp_http))
        .nest("/api", api)
        .with_state(store)
        // The Leptos UI. Unknown paths fall back to index.html so client routes survive a refresh.
        .fallback_service(ServeDir::new(&ui_dir).fallback(ServeFile::new(ui_dir.join("index.html"))))
        .layer(CompressionLayer::new())
        .layer(header(CONTENT_SECURITY_POLICY, APP_CSP))
        .layer(header(X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(header(X_FRAME_OPTIONS, "DENY"))
        .layer(header(REFERRER_POLICY, "no-referrer"))
        .layer(header(HeaderName::from_static("permissions-policy"), "camera=(), microphone=(), geolocation=()"))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT))
        .layer(TraceLayer::new_for_http())
        .layer(Extension(limits))
        .layer(Extension(oauth))
        .layer(Extension(mailer))
        .layer(Extension(ResetBase(public_url.trim_end_matches('/').to_string())))
}

/// Is the database reachable? Whatever watches the server (a proxy, an orchestrator) should ask this.
async fn health(State(s): State<Store>) -> Result<&'static str, StatusCode> {
    s.ping().await.map(|_| "ok").map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

async fn mcp_http(State(s): State<Store>, Auth(c, _): Auth, Json(req): Json<Value>) -> Response {
    // a batch or a single message
    match req {
        Value::Array(batch) => {
            let mut replies = Vec::new();
            for m in batch {
                replies.extend(mcp::handle(&s, &c, m).await);
            }
            if replies.is_empty() { StatusCode::ACCEPTED.into_response() } else { Json(Value::Array(replies)).into_response() }
        }
        m => match mcp::handle(&s, &c, m).await {
            Some(r) => Json(r).into_response(),
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

async fn sign_up(State(s): State<Store>, Extension(o): Extension<Arc<Oauth>>, Extension(l): Extension<Arc<Limits>>, ClientIp(ip): ClientIp, Json(b): Json<SignUp>) -> R<Session> {
    need_password(&o)?;
    l.sign_up.hit(&ip)?;
    Ok(Json(s.sign_up(b).await?))
}

async fn sign_in(State(s): State<Store>, Extension(o): Extension<Arc<Oauth>>, Extension(l): Extension<Arc<Limits>>, ClientIp(ip): ClientIp, Json(b): Json<SignIn>) -> R<Session> {
    need_password(&o)?;
    l.sign_in_ip.hit(&ip)?;
    l.sign_in.hit(&format!("{ip}|{}", b.email.trim().to_lowercase()))?;
    Ok(Json(s.sign_in(b).await?))
}

async fn sign_out(State(s): State<Store>, Auth(_, token): Auth) -> R<Value> {
    s.sign_out(&token).await?;
    Ok(Json(json!({ "ok": true })))
}

/// The providers people can sign in with here, so the app knows which buttons to show.
/// What the sign-in screens should offer besides the providers.
async fn options(Extension(o): Extension<Arc<Oauth>>) -> Json<Value> {
    Json(json!({ "password": o.password() }))
}

/// Refuse the email-and-password routes when the server only signs people in through a provider.
fn need_password(o: &Oauth) -> Result<(), ApiError> {
    if o.password() { Ok(()) } else { Err(Error::Forbidden("signing in with a password is turned off here: use google or github".into()).into()) }
}

async fn providers(Extension(o): Extension<Arc<Oauth>>) -> Json<Vec<&'static str>> {
    Json(o.enabled())
}

/// The cookie that ties a provider sign-in to the browser that started it.
const FLOW_COOKIE: &str = "pebblelab_oauth";

fn flow_cookie(o: &Oauth, value: &str, max_age: u32) -> String {
    format!("{FLOW_COOKIE}={value}; HttpOnly; SameSite=Lax; Path=/api/auth; Max-Age={max_age}{}", if o.secure() { "; Secure" } else { "" })
}

fn go(to: &str, cookie: Option<String>) -> Response {
    let mut r = (StatusCode::SEE_OTHER, [(LOCATION, to.to_string())]).into_response();
    if let Some(c) = cookie.and_then(|c| HeaderValue::from_str(&c).ok()) {
        r.headers_mut().append(SET_COOKIE, c);
    }
    r.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// Send the browser to the provider.
async fn oauth_start(State(s): State<Store>, Extension(o): Extension<Arc<Oauth>>, Extension(l): Extension<Arc<Limits>>, ClientIp(ip): ClientIp, Path(provider): Path<String>) -> Result<Response, ApiError> {
    l.sign_in_ip.hit(&ip)?;
    if !o.enabled().contains(&provider.as_str()) {
        return Err(Error::NotFound("provider").into());
    }
    let (state, verifier) = s.oauth_start(&provider).await?;
    let url = o.authorize_url(&provider, &state, &verifier)?;
    Ok(go(&url, Some(flow_cookie(&o, &state, 600))))
}

#[derive(Deserialize)]
struct Back {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// The provider sent the person back. Whatever happens they land in the app: signed in, or on the sign-in page
/// with the reason. The session never travels in the address, only a code that works once.
async fn oauth_callback(
    State(s): State<Store>,
    Extension(o): Extension<Arc<Oauth>>,
    Extension(l): Extension<Arc<Limits>>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Query(q): Query<Back>,
) -> Response {
    let outcome: Result<String, Error> = async {
        l.sign_in_ip.hit(&ip)?;
        if q.error.is_some() {
            return Err(Error::bad(format!("sign-in with {provider} was cancelled")));
        }
        let (Some(code), Some(state)) = (q.code.as_deref(), q.state.as_deref()) else {
            return Err(Error::bad("the provider did not send a code: start again"));
        };
        let theirs = headers
            .get_all(COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|c| c.trim().strip_prefix(&format!("{FLOW_COOKIE}=")).map(str::to_string))
            .next();
        if theirs.as_deref() != Some(state) {
            return Err(Error::bad("this sign-in did not start in this browser: start again"));
        }
        let verifier = s.oauth_finish(&provider, state).await?;
        let who = o.identity(&provider, code, &verifier).await?;
        s.external_sign_in(who).await
    }
    .await;
    let clear = Some(flow_cookie(&o, "", 0));
    match outcome {
        Ok(code) => go(&format!("/auth/callback#code={code}"), clear),
        Err(e) => go(&format!("/signin#{}", serde_urlencoded::to_string([("error", e.message())]).unwrap_or_default()), clear),
    }
}

#[derive(Deserialize)]
struct Redeem {
    code: String,
}

/// Swap the code from a provider sign-in for a session.
async fn redeem(State(s): State<Store>, Extension(l): Extension<Arc<Limits>>, ClientIp(ip): ClientIp, Json(b): Json<Redeem>) -> R<Session> {
    l.sign_in_ip.hit(&ip)?;
    Ok(Json(s.redeem_login_code(&b.code).await?))
}

#[derive(Deserialize)]
struct Reset {
    email: String,
}

/// Where the reset link points: the public address of this server.
#[derive(Clone)]
struct ResetBase(String);

/// Email a reset link. The answer is the same whether or not the address has an account, and the email goes
/// out after it, so neither the reply nor its timing says which.
async fn reset(
    State(s): State<Store>,
    Extension(o): Extension<Arc<Oauth>>,
    Extension(l): Extension<Arc<Limits>>,
    Extension(m): Extension<Arc<Mailer>>,
    Extension(ResetBase(base)): Extension<ResetBase>,
    ClientIp(ip): ClientIp,
    Json(b): Json<Reset>,
) -> R<Value> {
    need_password(&o)?;
    l.reset.hit(&ip)?;
    l.reset.hit(&format!("email:{}", b.email.trim().to_lowercase()))?;
    if !m.enabled() {
        tracing::warn!("password reset asked for but email is off: set PEBBLELAB_RESEND_API_KEY, or use `pebblelab user passwd`");
    } else {
        tokio::spawn(async move {
            match s.start_reset(&b.email).await {
                Ok(Some(t)) => {
                    // in the fragment, so it is never sent to a server, a log or a referer
                    if let Err(e) = m.password_reset(&t.email, &t.name, &format!("{base}/reset#token={}", t.token)).await {
                        tracing::error!("reset email failed: {e}");
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::error!("reset: {e}"),
            }
        });
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ResetConfirm {
    token: String,
    password: String,
}

async fn reset_confirm(State(s): State<Store>, Extension(o): Extension<Arc<Oauth>>, Extension(l): Extension<Arc<Limits>>, ClientIp(ip): ClientIp, Json(b): Json<ResetConfirm>) -> R<Value> {
    need_password(&o)?;
    l.sign_in_ip.hit(&ip)?;
    s.finish_reset(&b.token, &b.password).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn me(State(s): State<Store>, Auth(c, _): Auth) -> R<Me> {
    Ok(Json(s.me(&c).await?))
}

async fn update_me(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<UpdateProfile>) -> R<User> {
    Ok(Json(s.update_profile(&c, b).await?))
}

async fn password(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<ChangePassword>) -> R<Value> {
    s.change_password(&c, b).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn create_family(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<NewFamily>) -> R<Family> {
    Ok(Json(s.create_family(&c, b).await?))
}

async fn join_family(State(s): State<Store>, Extension(l): Extension<Arc<Limits>>, Auth(c, _): Auth, Json(b): Json<JoinFamily>) -> R<Family> {
    l.join.hit(&c.user_id.to_string())?;
    Ok(Json(s.join_family(&c, b).await?))
}

async fn delete_family(State(s): State<Store>, Auth(c, _): Auth) -> R<Value> {
    s.delete_family(&c).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn invite(State(s): State<Store>, Auth(c, _): Auth) -> R<Family> {
    Ok(Json(s.generate_invite(&c).await?))
}

async fn sign_out_all(State(s): State<Store>, Auth(c, _): Auth) -> R<Value> {
    s.sign_out_all(&c).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Password {
    password: String,
}

async fn delete_me(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<Password>) -> R<Value> {
    s.delete_user(&c, &b.password).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn export_csv(State(s): State<Store>, Auth(c, _): Auth) -> Result<Response, ApiError> {
    let csv = s.export_csv(&c).await?;
    Ok(([(CONTENT_TYPE, "text/csv; charset=utf-8"), (CONTENT_DISPOSITION, "attachment; filename=\"pebblelab-fin.csv\"")], csv).into_response())
}

async fn leave_account(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>) -> R<Value> {
    s.leave_account(&c, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_account(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>) -> R<Value> {
    s.delete_account(&c, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn update_connector(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>, Json(b): Json<UpdateConnector>) -> R<Connector> {
    Ok(Json(s.update_connector(&c, id, b).await?))
}

async fn leave_family(State(s): State<Store>, Auth(c, _): Auth) -> R<Value> {
    s.leave_family(&c).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct AccountQuery {
    #[serde(default)]
    include_archived: bool,
}

async fn accounts(State(s): State<Store>, Auth(c, _): Auth, Query(q): Query<AccountQuery>) -> R<Vec<Account>> {
    Ok(Json(s.accounts(&c, q.include_archived).await?))
}

async fn account(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>) -> R<Account> {
    Ok(Json(s.account(&c, id).await?))
}

async fn create_account(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<NewAccount>) -> R<Account> {
    Ok(Json(s.create_account(&c, b).await?))
}

async fn update_account(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>, Json(b): Json<UpdateAccount>) -> R<Account> {
    Ok(Json(s.update_account(&c, id, b).await?))
}

async fn transactions(State(s): State<Store>, Auth(c, _): Auth, Query(f): Query<TxFilter>) -> R<TxPage> {
    Ok(Json(s.transactions(&c, f).await?))
}

async fn transaction(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>) -> R<Transaction> {
    Ok(Json(s.transaction(&c, id).await?))
}

async fn add_transaction(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<NewTransaction>) -> R<Transaction> {
    Ok(Json(s.add_transaction(&c, b).await?))
}

async fn update_transaction(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>, Json(b): Json<UpdateTransaction>) -> R<Transaction> {
    Ok(Json(s.update_transaction(&c, id, b).await?))
}

async fn delete_transaction(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>) -> R<Value> {
    Ok(Json(json!({ "deleted": s.delete_transaction(&c, id).await? })))
}

async fn transfer(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<NewTransfer>) -> R<Vec<Transaction>> {
    Ok(Json(s.transfer(&c, b).await?))
}

#[derive(Deserialize)]
struct FileName {
    name: String,
}

/// The file is the raw request body; its name comes from `?name=` and its type from `Content-Type`.
async fn add_attachment(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>, Query(q): Query<FileName>, headers: HeaderMap, body: Bytes) -> R<Attachment> {
    let mime = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream");
    Ok(Json(s.add_attachment(&c, id, &q.name, mime, body.to_vec()).await?))
}

async fn attachment(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>) -> Result<Response, ApiError> {
    let (name, mime, data) = s.attachment(&c, id).await?;
    let safe: String = name.chars().filter(|ch| ch.is_ascii_graphic() && *ch != '"' && *ch != '\\').collect();
    // only pictures and pdfs are ever shown in place; anything else is a download, and none of it can run
    let how = if pebblelab_core::INLINE_TYPES.contains(&mime.as_str()) { "inline" } else { "attachment" };
    Ok((
        [
            (CONTENT_TYPE, mime),
            (CONTENT_DISPOSITION, format!("{how}; filename=\"{safe}\"")),
            (X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
            (CONTENT_SECURITY_POLICY, "sandbox; default-src 'none'; style-src 'unsafe-inline'".into()),
            (CACHE_CONTROL, "private, no-store".into()),
        ],
        data,
    )
        .into_response())
}

async fn delete_attachment(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>) -> R<Value> {
    s.delete_attachment(&c, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn tags(State(s): State<Store>, Auth(c, _): Auth) -> R<Vec<Value>> {
    Ok(Json(s.tags(&c).await?.into_iter().map(|(tag, uses)| json!({ "tag": tag, "uses": uses })).collect()))
}

async fn insights(State(s): State<Store>, Auth(c, _): Auth, Query(q): Query<InsightsQuery>) -> R<Insights> {
    Ok(Json(s.insights(&c, q).await?))
}

async fn ask(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<Ask>) -> R<Answer> {
    Ok(Json(Answer { answer: s.ask(&c, &b.question).await? }))
}

async fn notifications(State(s): State<Store>, Auth(c, _): Auth) -> R<Vec<Notification>> {
    Ok(Json(s.notifications(&c).await?))
}

#[derive(Deserialize, Default)]
struct ReadBody {
    id: Option<i64>,
}

async fn read_notifications(State(s): State<Store>, Auth(c, _): Auth, body: Option<Json<ReadBody>>) -> R<Value> {
    s.mark_notifications_read(&c, body.and_then(|b| b.0.id)).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn connectors(State(s): State<Store>, Auth(c, _): Auth) -> R<Vec<Connector>> {
    Ok(Json(s.connectors(&c).await?))
}

async fn create_connector(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<NewConnector>) -> R<CreatedConnector> {
    Ok(Json(s.create_connector(&c, b).await?))
}

async fn revoke_connector(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>) -> R<Value> {
    s.revoke_connector(&c, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn subscriptions(State(s): State<Store>, Auth(c, _): Auth) -> R<Vec<Subscription>> {
    Ok(Json(s.subscriptions(&c).await?))
}

async fn add_subscription(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<NewSubscription>) -> R<Subscription> {
    Ok(Json(s.add_subscription(&c, b).await?))
}

async fn update_subscription(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>, Json(b): Json<UpdateSubscription>) -> R<Subscription> {
    Ok(Json(s.update_subscription(&c, id, b).await?))
}

async fn delete_subscription(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>) -> R<Value> {
    s.delete_subscription(&c, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn assets(State(s): State<Store>, Auth(c, _): Auth) -> R<Vec<Asset>> {
    Ok(Json(s.assets(&c).await?))
}

async fn add_asset(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<NewAsset>) -> R<Asset> {
    Ok(Json(s.add_asset(&c, b).await?))
}

async fn update_asset(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>, Json(b): Json<UpdateAsset>) -> R<Asset> {
    Ok(Json(s.update_asset(&c, id, b).await?))
}

async fn delete_asset(State(s): State<Store>, Auth(c, _): Auth, Path(id): Path<i64>) -> R<Value> {
    s.delete_asset(&c, id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    async fn app() -> (Router, Store) {
        let store = Store::test().await.unwrap();
        let dir = std::env::temp_dir();
        (router(store.clone(), dir, Arc::new(Limits::new(true)), Arc::new(Oauth::none()), Arc::new(Mailer::off()), String::new()), store)
    }

    async fn call(app: &Router, method: &str, uri: &str, token: Option<&str>, body: Option<Value>, ip: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut b = Request::builder().method(method).uri(uri).header("x-forwarded-for", ip);
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        let req = match body {
            Some(v) => b.header("content-type", "application/json").body(Body::from(v.to_string())).unwrap(),
            None => b.body(Body::empty()).unwrap(),
        };
        let res = app.clone().oneshot(req).await.unwrap();
        let (status, headers) = (res.status(), res.headers().clone());
        (status, headers, res.into_body().collect().await.unwrap().to_bytes().to_vec())
    }

    async fn json(app: &Router, method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        let (s, _, b) = call(app, method, uri, token, body, "10.0.0.1").await;
        (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
    }

    async fn sign_up(app: &Router, name: &str) -> String {
        let (s, v) = json(app, "POST", "/api/auth/signup", None, Some(json!({"name": name, "email": format!("{name}@x.example"), "password": "correct horse battery"}))).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v["token"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn reset_answers_the_same_for_any_address_and_the_token_works_once() {
        let (app, store) = app().await;
        let old = sign_up(&app, "ria").await;
        for email in ["ria@x.example", "nobody@x.example"] {
            let (s, v) = json(&app, "POST", "/api/auth/reset", None, Some(json!({ "email": email }))).await;
            assert_eq!((s, v), (StatusCode::OK, json!({ "ok": true })));
        }
        let t = store.start_reset("RIA@x.example").await.unwrap().expect("account").token;
        assert!(store.start_reset("nobody@x.example").await.unwrap().is_none());
        let (s, _) = json(&app, "POST", "/api/auth/reset/confirm", None, Some(json!({ "token": t, "password": "short" }))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "a weak password must not spend the link");
        let (s, _) = json(&app, "POST", "/api/auth/reset/confirm", None, Some(json!({ "token": t, "password": "a brand new passphrase" }))).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _) = json(&app, "POST", "/api/auth/reset/confirm", None, Some(json!({ "token": t, "password": "another new passphrase" }))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "the link works once");
        let (s, _) = json(&app, "GET", "/api/me", Some(&old), None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "old sessions are signed out");
        let (s, _) = json(&app, "POST", "/api/auth/signin", None, Some(json!({ "email": "ria@x.example", "password": "a brand new passphrase" }))).await;
        assert_eq!(s, StatusCode::OK);
    }

    #[tokio::test]
    async fn with_password_login_off_only_providers_get_people_in() {
        let store = Store::test().await.unwrap();
        let app = router(store, std::env::temp_dir(), Arc::new(Limits::new(true)), Arc::new(Oauth::none().with_password(false)), Arc::new(Mailer::off()), String::new());
        let (s, v) = json(&app, "GET", "/api/auth/options", None, None).await;
        assert_eq!((s, v), (StatusCode::OK, json!({ "password": false })));
        for (path, body) in [
            ("/api/auth/signup", json!({"name": "a", "email": "a@x.example", "password": "correct horse battery"})),
            ("/api/auth/signin", json!({"email": "a@x.example", "password": "correct horse battery"})),
            ("/api/auth/reset", json!({"email": "a@x.example"})),
            ("/api/auth/reset/confirm", json!({"token": "x", "password": "correct horse battery"})),
        ] {
            let (s, _) = json(&app, "POST", path, None, Some(body)).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
        }
    }

    #[tokio::test]
    async fn every_data_route_needs_a_token() {
        let (app, _) = app().await;
        for (m, uri) in [("GET", "/api/me"), ("GET", "/api/accounts"), ("GET", "/api/transactions"), ("GET", "/api/insights"), ("GET", "/api/export.csv"), ("GET", "/api/attachments/1"), ("POST", "/mcp")] {
            let (s, _) = json(&app, m, uri, None, None).await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri}");
        }
        let (s, _) = json(&app, "GET", "/api/me", Some("trs_nonsense"), None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, v) = json(&app, "GET", "/api/nope", None, None).await;
        assert_eq!((s, v["error"].as_str()), (StatusCode::NOT_FOUND, Some("no such route")), "an unknown api path is not the web app");
    }

    #[tokio::test]
    async fn people_only_see_their_own_data() {
        let (app, _) = app().await;
        let (a, b) = (sign_up(&app, "anita").await, sign_up(&app, "vikram").await);
        let (s, acct) = json(&app, "POST", "/api/accounts", Some(&a), Some(json!({"name": "salary", "kind": "bank", "balance": "100"}))).await;
        assert_eq!(s, StatusCode::OK, "{acct}");
        let id = acct["id"].as_i64().unwrap();
        let (s, _) = json(&app, "POST", "/api/transactions", Some(&a), Some(json!({"account_id": id, "kind": "debit", "amount": "5"}))).await;
        assert_eq!(s, StatusCode::OK);
        // vikram cannot read, change or delete any of it
        assert_eq!(json(&app, "GET", &format!("/api/accounts/{id}"), Some(&b), None).await.0, StatusCode::NOT_FOUND);
        assert_eq!(json(&app, "PATCH", &format!("/api/accounts/{id}"), Some(&b), Some(json!({"name": "mine now"}))).await.0, StatusCode::NOT_FOUND);
        assert_eq!(json(&app, "DELETE", &format!("/api/accounts/{id}"), Some(&b), None).await.0, StatusCode::NOT_FOUND);
        assert_eq!(json(&app, "POST", "/api/transactions", Some(&b), Some(json!({"account_id": id, "kind": "credit", "amount": "1"}))).await.0, StatusCode::NOT_FOUND);
        let (_, page) = json(&app, "GET", "/api/transactions", Some(&b), None).await;
        assert_eq!(page["total"], 0);
        let (_, mine) = json(&app, "GET", "/api/transactions", Some(&a), None).await;
        assert_eq!(mine["total"], 1);
    }

    #[tokio::test]
    async fn a_connector_is_held_to_its_scopes() {
        let (app, _) = app().await;
        let a = sign_up(&app, "anita").await;
        let (_, acct) = json(&app, "POST", "/api/accounts", Some(&a), Some(json!({"name": "salary", "kind": "bank"}))).await;
        let id = acct["id"].as_i64().unwrap();
        let (_, made) = json(&app, "POST", "/api/connectors", Some(&a), Some(json!({"name": "claude", "scopes": ["read"]}))).await;
        let t = made["token"].as_str().unwrap();
        assert_eq!(json(&app, "GET", "/api/accounts", Some(t), None).await.0, StatusCode::OK);
        assert_eq!(json(&app, "GET", "/api/transactions", Some(t), None).await.0, StatusCode::FORBIDDEN);
        assert_eq!(json(&app, "POST", "/api/transactions", Some(t), Some(json!({"account_id": id, "kind": "debit", "amount": "1"}))).await.0, StatusCode::FORBIDDEN);
        assert_eq!(json(&app, "DELETE", &format!("/api/accounts/{id}"), Some(t), None).await.0, StatusCode::FORBIDDEN);
        assert_eq!(json(&app, "POST", "/api/connectors", Some(t), Some(json!({"name": "more"}))).await.0, StatusCode::FORBIDDEN, "a read token cannot mint a stronger one");
        // over mcp it is offered only what it can use
        let (_, tools) = json(&app, "POST", "/mcp", Some(t), Some(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))).await;
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"list_accounts") && !names.contains(&"add_transaction") && !names.contains(&"delete_transaction"), "{names:?}");
    }

    #[tokio::test]
    async fn guessing_passwords_is_slowed_down() {
        let (app, _) = app().await;
        sign_up(&app, "anita").await;
        let attempt = |ip: &'static str| {
            let app = app.clone();
            async move { call(&app, "POST", "/api/auth/signin", None, Some(json!({"email": "anita@x.example", "password": "wrong wrong wrong"})), ip).await.0 }
        };
        for _ in 0..8 {
            assert_eq!(attempt("9.9.9.9").await, StatusCode::UNAUTHORIZED);
        }
        assert_eq!(attempt("9.9.9.9").await, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(attempt("8.8.8.8").await, StatusCode::UNAUTHORIZED, "someone else is not held back");
        let (s, _, _) = call(&app, "POST", "/api/auth/signin", None, Some(json!({"email": "anita@x.example", "password": "correct horse battery"})), "9.9.9.9").await;
        assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "even the right password waits out the window");
    }

    #[tokio::test]
    async fn closed_sign_ups_are_refused_over_rest() {
        let (_, store) = app().await;
        let app = router(store.with_config(pebblelab_core::Config { signups_open: false, ..Default::default() }), std::env::temp_dir(), Arc::new(Limits::new(false)), Arc::new(Oauth::none()), Arc::new(Mailer::off()), String::new());
        let (s, _) = json(&app, "POST", "/api/auth/signup", None, Some(json!({"name": "x", "email": "x@y.example", "password": "correct horse battery"}))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn attachments_never_run_and_everything_carries_security_headers() {
        let (app, _) = app().await;
        let a = sign_up(&app, "anita").await;
        let (_, acct) = json(&app, "POST", "/api/accounts", Some(&a), Some(json!({"name": "salary", "kind": "bank"}))).await;
        let (_, tx) = json(&app, "POST", "/api/transactions", Some(&a), Some(json!({"account_id": acct["id"], "kind": "debit", "amount": "1"}))).await;
        let up = |mime: &'static str| {
            let app = app.clone();
            let (a, id) = (a.clone(), tx["id"].as_i64().unwrap());
            async move {
                let req = Request::builder().method("POST").uri(format!("/api/transactions/{id}/attachments?name=r")).header("authorization", format!("Bearer {a}")).header("content-type", mime).body(Body::from("<script>alert(1)</script>")).unwrap();
                let res = app.oneshot(req).await.unwrap();
                serde_json::from_slice::<Value>(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()["id"].as_i64().unwrap()
            }
        };
        let html = up("text/html").await;
        let (s, h, _) = call(&app, "GET", &format!("/api/attachments/{html}"), Some(&a), None, "1.1.1.1").await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(h[CONTENT_TYPE], "application/octet-stream");
        assert!(h[CONTENT_DISPOSITION].to_str().unwrap().starts_with("attachment"));
        assert!(h[CONTENT_SECURITY_POLICY].to_str().unwrap().contains("sandbox"));
        let png = up("image/png").await;
        let (_, h, _) = call(&app, "GET", &format!("/api/attachments/{png}"), Some(&a), None, "1.1.1.1").await;
        assert_eq!(h[CONTENT_TYPE], "image/png");
        assert!(h[CONTENT_DISPOSITION].to_str().unwrap().starts_with("inline"));

        let (_, h, _) = call(&app, "GET", "/health", None, None, "1.1.1.1").await;
        assert!(h[CONTENT_SECURITY_POLICY].to_str().unwrap().contains("frame-ancestors 'none'"));
        assert_eq!(h[X_FRAME_OPTIONS], "DENY");
        assert_eq!(h[X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(call(&app, "GET", "/health", None, None, "1.1.1.1").await.0, StatusCode::OK);
    }

    // ---- signing in with Google and GitHub, against a stand-in for the provider ----------------------

    use std::collections::HashMap;
    use std::sync::Mutex;

    use axum::routing::{get, post};
    use base64::Engine;
    use sha2::{Digest, Sha256};

    use crate::oauth::Provider;

    /// What the stand-in provider says about whoever signs in, and what it was sent.
    #[derive(Default)]
    struct Stand {
        profile: Mutex<Value>,
        emails: Mutex<Value>,
        verifier: Mutex<String>,
    }

    /// A provider on a local port: `/token` accepts the code `good`, `/me` and `/emails` say who it is.
    async fn stand_in(state: Arc<Stand>) -> String {
        async fn token(State(st): State<Arc<Stand>>, axum::Form(f): axum::Form<HashMap<String, String>>) -> Json<Value> {
            *st.verifier.lock().unwrap() = f.get("code_verifier").cloned().unwrap_or_default();
            if f.get("code").map(String::as_str) == Some("good") && f.get("client_secret").map(String::as_str) == Some("secret") {
                Json(json!({"access_token": "at"}))
            } else {
                Json(json!({"error": "bad_verification_code"}))
            }
        }
        async fn me(State(st): State<Arc<Stand>>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
            if headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) != Some("Bearer at") {
                return Err(StatusCode::UNAUTHORIZED);
            }
            Ok(Json(st.profile.lock().unwrap().clone()))
        }
        async fn emails(State(st): State<Arc<Stand>>) -> Json<Value> {
            Json(st.emails.lock().unwrap().clone())
        }
        let app = Router::new().route("/token", post(token)).route("/me", get(me)).route("/emails", get(emails)).with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    fn aimed_at(mut p: Provider, base: &str) -> Provider {
        p.auth_url = format!("{base}/authorize");
        p.token_url = format!("{base}/token");
        p.userinfo_url = format!("{base}/me");
        p.emails_url = format!("{base}/emails");
        p
    }

    fn header_of(h: &HeaderMap, name: &str) -> String {
        h.get_all(name).iter().map(|v| v.to_str().unwrap().to_string()).collect::<Vec<_>>().join(" | ")
    }

    async fn with_providers(store: Store, base: &str) -> Router {
        let mut providers = HashMap::new();
        providers.insert("google", aimed_at(Provider::google("gid".into(), "secret".into()), base));
        providers.insert("github", aimed_at(Provider::github("hid".into(), "secret".into()), base));
        router(store, std::env::temp_dir(), Arc::new(Limits::new(true)), Arc::new(Oauth::new("http://pebblelab.test", providers)), Arc::new(Mailer::off()), String::new())
    }

    /// Start a sign-in the way a browser would, and return its state and cookie.
    async fn begin(app: &Router, provider: &str, stand: &Stand) -> (String, String) {
        let (s, h, _) = call(app, "GET", &format!("/api/auth/{provider}/start"), None, None, "2.2.2.2").await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        let to = header_of(&h, "location");
        let query: HashMap<String, String> = serde_urlencoded::from_str(to.split_once('?').unwrap().1).unwrap();
        assert_eq!(query["redirect_uri"], format!("http://pebblelab.test/api/auth/{provider}/callback"));
        assert_eq!(query["code_challenge_method"], "S256");
        let cookie = header_of(&h, "set-cookie");
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax") && cookie.contains(&format!("pebblelab_oauth={}", query["state"])), "{cookie}");
        let _ = stand;
        (query["state"].clone(), query["code_challenge"].clone())
    }

    async fn come_back(app: &Router, provider: &str, query: &str, cookie: Option<&str>) -> String {
        let mut b = Request::builder().method("GET").uri(format!("/api/auth/{provider}/callback?{query}")).header("x-forwarded-for", "2.2.2.2");
        if let Some(c) = cookie {
            b = b.header("cookie", c);
        }
        let res = app.clone().oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        header_of(res.headers(), "location")
    }

    #[tokio::test]
    async fn signing_in_with_google_end_to_end() {
        let stand = Arc::new(Stand::default());
        *stand.profile.lock().unwrap() = json!({"sub": "g-1", "email": "anita@x.example", "email_verified": true, "name": "Anita Rao"});
        let base = stand_in(stand.clone()).await;
        let (_, store) = app().await;
        let app = with_providers(store, &base).await;

        let (_, providers) = json(&app, "GET", "/api/auth/providers", None, None).await;
        assert_eq!(providers, json!(["github", "google"]));

        let (state, challenge) = begin(&app, "google", &stand).await;
        let cookie = format!("other=1; pebblelab_oauth={state}");
        let to = come_back(&app, "google", &format!("code=good&state={state}"), Some(&cookie)).await;
        let code = to.strip_prefix("/auth/callback#code=").unwrap_or_else(|| panic!("{to}"));
        // the provider was shown the proof for the challenge the browser was sent
        let shown = stand.verifier.lock().unwrap().clone();
        assert_eq!(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(shown.as_bytes())), challenge);

        let (s, session) = json(&app, "POST", "/api/auth/redeem", None, Some(json!({"code": code}))).await;
        assert_eq!(s, StatusCode::OK, "{session}");
        let token = session["token"].as_str().unwrap();
        let (_, me) = json(&app, "GET", "/api/me", Some(token), None).await;
        assert_eq!(me["email"], "anita@x.example");
        assert_eq!(json(&app, "POST", "/api/auth/redeem", None, Some(json!({"code": code}))).await.0, StatusCode::UNAUTHORIZED, "the code works once");

        // the state works once too
        let to = come_back(&app, "google", &format!("code=good&state={state}"), Some(&cookie)).await;
        assert!(to.starts_with("/signin#error="), "{to}");
    }

    #[tokio::test]
    async fn a_provider_sign_in_that_goes_wrong_ends_on_the_sign_in_page_with_the_reason() {
        let stand = Arc::new(Stand::default());
        *stand.profile.lock().unwrap() = json!({"sub": "g-9", "email": "nobody@x.example", "email_verified": false});
        let base = stand_in(stand.clone()).await;
        let (_, store) = app().await;
        let app = with_providers(store, &base).await;

        let (state, _) = begin(&app, "google", &stand).await;
        let mine = format!("pebblelab_oauth={state}");
        // no cookie, or someone else's: the sign-in did not start in this browser
        for cookie in [None, Some("pebblelab_oauth=forged")] {
            let to = come_back(&app, "google", &format!("code=good&state={state}"), cookie).await;
            assert!(to.starts_with("/signin#error=") && to.contains("this+sign-in+did+not+start"), "{to}");
        }
        // cancelled at the provider
        let to = come_back(&app, "google", &format!("error=access_denied&state={state}"), Some(&mine)).await;
        assert!(to.contains("cancelled"), "{to}");
        // a bad code, then an unverified email
        let (state, _) = begin(&app, "google", &stand).await;
        let mine = format!("pebblelab_oauth={state}");
        assert!(come_back(&app, "google", &format!("code=forged&state={state}"), Some(&mine)).await.starts_with("/signin#error="));
        let (state, _) = begin(&app, "google", &stand).await;
        let to = come_back(&app, "google", &format!("code=good&state={state}"), Some(&format!("pebblelab_oauth={state}"))).await;
        assert!(to.contains("verified"), "{to}");
        // a provider that is not set up
        assert_eq!(json(&app, "GET", "/api/auth/myspace/start", None, None).await.0, StatusCode::NOT_FOUND);
        let (_, none) = app_without_providers().await;
        assert_eq!(json(&none, "GET", "/api/auth/google/start", None, None).await.0, StatusCode::NOT_FOUND);
    }

    async fn app_without_providers() -> ((), Router) {
        let (app, _) = app().await;
        ((), app)
    }

    #[tokio::test]
    async fn signing_in_with_github_uses_the_verified_primary_email() {
        let stand = Arc::new(Stand::default());
        *stand.profile.lock().unwrap() = json!({"id": 4242, "login": "anita-rao", "name": null});
        *stand.emails.lock().unwrap() = json!([
            {"email": "old@x.example", "primary": false, "verified": true},
            {"email": "anita@x.example", "primary": true, "verified": true},
            {"email": "spoof@x.example", "primary": false, "verified": false}
        ]);
        let base = stand_in(stand.clone()).await;
        let (_, store) = app().await;
        let app = with_providers(store, &base).await;
        let (state, _) = begin(&app, "github", &stand).await;
        let to = come_back(&app, "github", &format!("code=good&state={state}"), Some(&format!("pebblelab_oauth={state}"))).await;
        let code = to.strip_prefix("/auth/callback#code=").unwrap_or_else(|| panic!("{to}"));
        let (_, session) = json(&app, "POST", "/api/auth/redeem", None, Some(json!({"code": code}))).await;
        assert_eq!(session["user"]["email"], "anita@x.example");
        assert_eq!(session["user"]["name"], "anita-rao", "the login stands in when there is no name");

        // only an unverified address: refused
        *stand.emails.lock().unwrap() = json!([{"email": "anita2@x.example", "primary": true, "verified": false}]);
        *stand.profile.lock().unwrap() = json!({"id": 777, "login": "someone"});
        let (state, _) = begin(&app, "github", &stand).await;
        let to = come_back(&app, "github", &format!("code=good&state={state}"), Some(&format!("pebblelab_oauth={state}"))).await;
        assert!(to.starts_with("/signin#error=") && to.contains("verified"), "{to}");
    }
}

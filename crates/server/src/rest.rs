use std::path::PathBuf;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State};
use axum::http::header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tracer_core::api::*;
use tracer_core::{Caller, Error, Store};

use crate::mcp;

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

pub fn router(store: Store, ui_dir: PathBuf) -> Router {
    let api = Router::new()
        .route("/auth/signup", post(sign_up))
        .route("/auth/signin", post(sign_in))
        .route("/auth/signout", post(sign_out))
        .route("/auth/signout-all", post(sign_out_all))
        .route("/auth/reset", post(reset))
        .route("/me", get(me).patch(update_me).delete(delete_me))
        .route("/export.csv", get(export_csv))
        .route("/me/password", post(password))
        .route("/family", post(create_family).delete(delete_family))
        .route("/family/invite", post(invite))
        .route("/family/leave", post(leave_family))
        .route("/family/join", post(join_family))
        .route("/accounts", get(accounts).post(create_account))
        .route("/accounts/{id}", get(account).patch(update_account).delete(delete_account))
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
        .layer(DefaultBodyLimit::max(12 * 1024 * 1024));
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/mcp", post(mcp_http))
        .nest("/api", api)
        .with_state(store)
        // The Leptos UI. Unknown paths fall back to index.html so client routes survive a refresh.
        .fallback_service(ServeDir::new(&ui_dir).fallback(ServeFile::new(ui_dir.join("index.html"))))
        .layer(CompressionLayer::new())
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

async fn sign_up(State(s): State<Store>, Json(b): Json<SignUp>) -> R<Session> {
    Ok(Json(s.sign_up(b).await?))
}

async fn sign_in(State(s): State<Store>, Json(b): Json<SignIn>) -> R<Session> {
    Ok(Json(s.sign_in(b).await?))
}

async fn sign_out(State(s): State<Store>, Auth(_, token): Auth) -> R<Value> {
    s.sign_out(&token).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Reset {
    #[allow(dead_code)]
    email: String,
}

/// There is no email service behind this yet, so it never reveals whether the address exists and sends
/// nothing. A reset is `tracer user passwd <email>` on the server.
async fn reset(Json(_): Json<Reset>) -> (StatusCode, Json<Value>) {
    (StatusCode::ACCEPTED, Json(json!({ "ok": true })))
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

async fn join_family(State(s): State<Store>, Auth(c, _): Auth, Json(b): Json<JoinFamily>) -> R<Family> {
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
    Ok(([(CONTENT_TYPE, "text/csv; charset=utf-8"), (CONTENT_DISPOSITION, "attachment; filename=\"tracer-fin.csv\"")], csv).into_response())
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
    Ok((
        [
            (CONTENT_TYPE, mime),
            (CONTENT_DISPOSITION, format!("inline; filename=\"{safe}\"")),
            (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
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

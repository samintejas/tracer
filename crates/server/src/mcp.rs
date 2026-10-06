//! MCP (Model Context Protocol) over the same store the REST API and CLI use. One implementation, two
//! transports: streamable HTTP at `POST /mcp` and newline-delimited JSON-RPC on stdio (`tracer mcp`).

use serde::Deserialize;
use serde_json::{Value, json};
use tracer_core::api::*;
use tracer_core::{Caller, Error, Store};

const NOTE: &str = "Amounts are decimal numbers or strings in major units (e.g. 3240 or \"3240.50\"). Dates are YYYY-MM-DD and default to today. Tags are lowercase words; the first tag is the category.";

fn tools() -> Value {
    let id = json!({"type": "integer"});
    let s = json!({"type": "string"});
    let amount = json!({"type": ["string", "number"], "description": "Positive amount in major units, e.g. 3240.50"});
    let date = json!({"type": "string", "description": "YYYY-MM-DD, defaults to today"});
    let tags = json!({"type": "array", "items": {"type": "string"}, "description": "lowercase tags; the first is the category, e.g. [\"groceries\", \"household\"]"});
    json!([
      {"name": "list_accounts", "description": format!("List the accounts you can see (yours, joint ones, and family-shared ones). Each has a balance: what you hold, or what you owe for credit and loan accounts (see `kind`). {NOTE}"),
       "inputSchema": {"type": "object", "properties": {"include_archived": {"type": "boolean"}}}},
      {"name": "create_account", "description": format!("Create an account. kind is bank, credit, loan or investment. `balance` is what you hold now, or owe now for credit. A loan needs loan_total, rate (annual %), tenure (months) and start (YYYY-MM); its balance comes from the schedule. owner_ids adds family members as co-owners of a bank account (joint); other kinds cannot be joint. {NOTE}"),
       "inputSchema": {"type": "object", "required": ["name", "kind"], "properties": {
         "name": s, "kind": {"type": "string", "enum": ACCOUNT_KINDS}, "balance": amount,
         "visibility": {"type": "string", "enum": ["private", "shared"], "description": "shared lets your family see it"},
         "owner_ids": {"type": "array", "items": id}, "institution": s, "last4": s,
         "limit": amount, "statement_day": id, "due_day": id,
         "loan_total": amount, "rate": {"type": "number"}, "tenure": id, "start": s, "emi": amount, "emi_day": id,
         "invest_kind": s, "invested": amount, "sip": amount}}},
      {"name": "update_account", "description": "Rename, archive, change visibility or owners, set the current balance (history is kept), or change details such as a credit limit or a loan's terms. Only owners can.",
       "inputSchema": {"type": "object", "required": ["account_id"], "properties": {
         "account_id": id, "name": s, "archived": {"type": "boolean"}, "visibility": {"type": "string", "enum": ["private", "shared"]},
         "owner_ids": {"type": "array", "items": id}, "balance": amount, "limit": amount, "due_day": id, "emi": amount, "invested": amount}}},
      {"name": "list_transactions", "description": "Search transactions you can see, newest first, with a total for paging. accounts is a comma separated list of account ids; kinds is debit, credit and/or transfer; tags matches any.",
       "inputSchema": {"type": "object", "properties": {
         "q": {"type": "string", "description": "substring of description, note or a tag"}, "account_id": id, "accounts": s, "member_id": id,
         "kinds": s, "tags": s, "collapse_transfers": {"type": "boolean", "description": "show each transfer once"}, "from": date, "to": date, "sort": {"type": "string", "enum": ["date", "description", "amount"]},
         "dir": {"type": "string", "enum": ["asc", "desc"]}, "limit": id, "offset": id}}},
      {"name": "add_transaction", "description": format!("Record money out (debit) or in (credit) on an account you own. A credit-card purchase is a debit on the card and raises what is owed. To move money between accounts, or pay a card or loan, use transfer_money instead. {NOTE}"),
       "inputSchema": {"type": "object", "required": ["account_id", "kind", "amount"], "properties": {
         "account_id": id, "kind": {"type": "string", "enum": ["debit", "credit"]}, "amount": amount,
         "description": s, "tags": tags, "note": s, "date": date}}},
      {"name": "transfer_money", "description": format!("Move money between two accounts as two linked transactions (pay a card or loan, fund an investment, top up savings). Both balances update. {NOTE}"),
       "inputSchema": {"type": "object", "required": ["from_account_id", "to_account_id", "amount"], "properties": {
         "from_account_id": id, "to_account_id": id, "amount": amount, "description": s, "tags": tags, "note": s, "date": date}}},
      {"name": "update_transaction", "description": "Edit a transaction; for a transfer both legs change. Pass a positive amount: the direction stays.",
       "inputSchema": {"type": "object", "required": ["transaction_id"], "properties": {
         "transaction_id": id, "amount": amount, "description": s, "tags": tags, "note": s, "date": date, "account_id": id}}},
      {"name": "delete_transaction", "description": "Delete a transaction (both legs if it is a transfer).",
       "inputSchema": {"type": "object", "required": ["transaction_id"], "properties": {"transaction_id": id}}},
      {"name": "list_subscriptions", "description": format!("List your subscriptions: standing charges on an account. When a renewal date arrives a transaction is added and the date moves on a month or a year. {NOTE}"),
       "inputSchema": {"type": "object", "properties": {}}},
      {"name": "add_subscription", "description": format!("Add a subscription paid from an account you own. cycle is monthly or yearly; next is the next renewal date (a past date is added to transactions straight away). tag is the category its transactions get. {NOTE}"),
       "inputSchema": {"type": "object", "required": ["name", "amount", "account_id"], "properties": {
         "name": s, "amount": amount, "cycle": {"type": "string", "enum": ["monthly", "yearly"]}, "next": date, "account_id": id, "tag": s, "active": {"type": "boolean"}}}},
      {"name": "update_subscription", "description": "Change a subscription, pause it (active: false) or resume it. next accepts a date, or an empty string to clear it.",
       "inputSchema": {"type": "object", "required": ["subscription_id"], "properties": {
         "subscription_id": id, "name": s, "amount": amount, "cycle": {"type": "string", "enum": ["monthly", "yearly"]}, "next": s, "account_id": id, "tag": s, "active": {"type": "boolean"}}}},
      {"name": "delete_subscription", "description": "Delete a subscription. Transactions it already added stay.",
       "inputSchema": {"type": "object", "required": ["subscription_id"], "properties": {"subscription_id": id}}},
      {"name": "list_assets", "description": format!("List things you own outside your accounts (property, vehicle, gold, electronics, other) with price paid and value now. They count towards assets in insights. {NOTE}"),
       "inputSchema": {"type": "object", "properties": {}}},
      {"name": "add_asset", "description": format!("Add something you own. kind is property, vehicle, gold, electronics or other; bought is a month as YYYY-MM; cost is what you paid, value is what it is worth now. {NOTE}"),
       "inputSchema": {"type": "object", "required": ["name", "value"], "properties": {
         "name": s, "kind": {"type": "string", "enum": ASSET_KINDS}, "bought": s, "cost": amount, "value": amount, "note": s}}},
      {"name": "update_asset", "description": "Change an asset, for example its value now.",
       "inputSchema": {"type": "object", "required": ["asset_id"], "properties": {
         "asset_id": id, "name": s, "kind": {"type": "string", "enum": ASSET_KINDS}, "bought": s, "cost": amount, "value": amount, "note": s}}},
      {"name": "delete_asset", "description": "Delete an asset.",
       "inputSchema": {"type": "object", "required": ["asset_id"], "properties": {"asset_id": id}}},
      {"name": "list_tags", "description": "Tags in use with counts, most used first. Reuse them rather than inventing near-duplicates.",
       "inputSchema": {"type": "object", "properties": {}}},
      {"name": "get_insights", "description": format!("Money overview: assets (accounts plus things you own), what is owed, income and spending over a window, spending by category, six months of flow, upcoming dues, loans and investments. member_id looks at one person. {NOTE}"),
       "inputSchema": {"type": "object", "properties": {"member_id": id, "days": {"type": "integer", "description": "window for totals, default 30"}}}},
      {"name": "ask_tracer", "description": "Ask a plain question (loans ending, what is due, investments, net worth, spending on a tag) and get an answer computed from the data.",
       "inputSchema": {"type": "object", "required": ["question"], "properties": {"question": s}}}
    ])
}

#[derive(Deserialize)]
struct AccountRef<T> {
    account_id: i64,
    #[serde(flatten)]
    rest: T,
}

#[derive(Deserialize)]
struct TxRef<T> {
    transaction_id: i64,
    #[serde(flatten)]
    rest: T,
}

#[derive(Deserialize)]
struct SubRef<T> {
    subscription_id: i64,
    #[serde(flatten)]
    rest: T,
}

#[derive(Deserialize)]
struct AssetRef<T> {
    asset_id: i64,
    #[serde(flatten)]
    rest: T,
}

#[derive(Deserialize, Default)]
struct Archived {
    #[serde(default)]
    include_archived: bool,
}

fn args<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(|e| Error::bad(format!("invalid arguments: {e}")))
}

fn out<T: serde::Serialize>(v: T) -> Result<Value, Error> {
    serde_json::to_value(v).map_err(|e| Error::Internal(e.to_string()))
}

async fn call_tool(s: &Store, c: &Caller, name: &str, a: Value) -> Result<Value, Error> {
    match name {
        "list_accounts" => out(s.accounts(c, args::<Archived>(a)?.include_archived).await?),
        "create_account" => out(s.create_account(c, args(a)?).await?),
        "update_account" => {
            let r: AccountRef<UpdateAccount> = args(a)?;
            out(s.update_account(c, r.account_id, r.rest).await?)
        }
        "list_transactions" => out(s.transactions(c, args(a)?).await?),
        "add_transaction" => out(s.add_transaction(c, args(a)?).await?),
        "transfer_money" => out(s.transfer(c, args(a)?).await?),
        "update_transaction" => {
            let r: TxRef<UpdateTransaction> = args(a)?;
            out(s.update_transaction(c, r.transaction_id, r.rest).await?)
        }
        "delete_transaction" => {
            let r: TxRef<serde_json::Map<String, Value>> = args(a)?;
            out(json!({"deleted": s.delete_transaction(c, r.transaction_id).await?}))
        }
        "list_subscriptions" => out(s.subscriptions(c).await?),
        "add_subscription" => out(s.add_subscription(c, args(a)?).await?),
        "update_subscription" => {
            let r: SubRef<UpdateSubscription> = args(a)?;
            out(s.update_subscription(c, r.subscription_id, r.rest).await?)
        }
        "delete_subscription" => {
            let r: SubRef<serde_json::Map<String, Value>> = args(a)?;
            s.delete_subscription(c, r.subscription_id).await?;
            out(json!({"deleted": true}))
        }
        "list_assets" => out(s.assets(c).await?),
        "add_asset" => out(s.add_asset(c, args(a)?).await?),
        "update_asset" => {
            let r: AssetRef<UpdateAsset> = args(a)?;
            out(s.update_asset(c, r.asset_id, r.rest).await?)
        }
        "delete_asset" => {
            let r: AssetRef<serde_json::Map<String, Value>> = args(a)?;
            s.delete_asset(c, r.asset_id).await?;
            out(json!({"deleted": true}))
        }
        "list_tags" => out(s.tags(c).await?.into_iter().map(|(tag, uses)| json!({"tag": tag, "uses": uses})).collect::<Vec<_>>()),
        "get_insights" => out(s.insights(c, args(a)?).await?),
        "ask_tracer" => out(json!({"answer": s.ask(c, &args::<Ask>(a)?.question).await?})),
        _ => Err(Error::bad(format!("unknown tool '{name}'"))),
    }
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn err(id: Value, code: i32, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

/// Handle one JSON-RPC message. `None` means it was a notification and gets no reply.
pub async fn handle(s: &Store, c: &Caller, req: Value) -> Option<Value> {
    let id = req.get("id").cloned()?;
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let who = s.user(c.user_id).await.map(|u| u.name).unwrap_or_default();
            ok(id, json!({
                "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!("2025-03-26")),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "tracer", "version": env!("CARGO_PKG_VERSION")},
                "instructions": format!("tracer/fin, the money of {who} and their family. Token scopes: {}. {NOTE}", c.scopes().join(", ")),
            }))
        }
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({"tools": tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let a = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            // failures are reported in-band so the model can read the message and correct itself
            let (text, is_error) = match call_tool(s, c, name, a).await {
                Ok(v) => (serde_json::to_string_pretty(&v).unwrap_or_default(), false),
                Err(e) => (e.message(), true),
            };
            ok(id, json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
        }
        _ => err(id, -32601, "method not found"),
    })
}

/// MCP over stdio: one JSON-RPC message per line in, one per line out.
pub async fn serve_stdio(s: Store, c: Caller) -> std::io::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let mut replies = Vec::new();
                for m in batch {
                    replies.extend(handle(&s, &c, m).await);
                }
                (!replies.is_empty()).then(|| Value::Array(replies))
            }
            Ok(m) => handle(&s, &c, m).await,
            Err(_) => Some(err(Value::Null, -32700, "parse error")),
        };
        if let Some(r) = reply {
            stdout.write_all(r.to_string().as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

use serde::{Deserialize, Serialize};

use crate::deposit::DepositCalc;
use crate::loan::LoanCalc;
use crate::money;

// ---- people --------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub initials: String,
    #[serde(default)]
    pub phone: String,
    /// `inr`, `usd` or `eur`: how amounts are written. The stored numbers do not change.
    pub currency: String,
    /// Profile picture as a `data:image/…` url, when one is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    /// Remind about card due dates.
    #[serde(default)]
    pub notify_card: bool,
    /// Remind about loan emis.
    #[serde(default)]
    pub notify_emi: bool,
    /// Tell me when someone else adds to an account I can see.
    #[serde(default)]
    pub notify_joint: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub id: i64,
    pub name: String,
    pub initials: String,
    /// Filled in family listings.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Family {
    pub id: i64,
    pub name: String,
    pub owner_id: i64,
    /// A one-time code, present after the owner generates one and until someone uses it.
    pub invite_code: Option<String>,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Me {
    #[serde(flatten)]
    pub user: User,
    pub family: Option<Family>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SignUp {
    pub name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SignIn {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub user: User,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateProfile {
    pub name: Option<String>,
    pub initials: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub currency: Option<String>,
    /// A `data:image/…` url, or an empty string to remove the picture.
    pub picture: Option<String>,
    pub notify_card: Option<bool>,
    pub notify_emi: Option<bool>,
    pub notify_joint: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangePassword {
    pub current: String,
    pub new: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewFamily {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JoinFamily {
    pub code: String,
}

// ---- accounts ------------------------------------------------------------------------------------------

pub const ACCOUNT_KINDS: [&str; 4] = ["bank", "credit", "loan", "investment"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Bank,
    Credit,
    Loan,
    Investment,
}

impl AccountKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountKind::Bank => "bank",
            AccountKind::Credit => "credit",
            AccountKind::Loan => "loan",
            AccountKind::Investment => "investment",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "bank" => AccountKind::Bank,
            "credit" => AccountKind::Credit,
            "loan" => AccountKind::Loan,
            "investment" => AccountKind::Investment,
            _ => return None,
        })
    }

    /// Money you owe rather than money you hold.
    pub fn is_liability(self) -> bool {
        matches!(self, AccountKind::Credit | AccountKind::Loan)
    }
}

/// What an investment is. The group decides which details it asks for and what can be worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvestGroup {
    /// Worth whatever the market says: you record the current value, and may add every month.
    Market,
    /// A fixed or recurring deposit: a rate, a start and a term give its worth now and at maturity.
    Deposit,
    /// Provident funds and pensions: the statement's value, a monthly contribution, an optional rate.
    Retirement,
    /// A holding that can also be one of your assets, so its value follows that asset.
    Physical,
    Other,
}

/// Every investment type, with its group. This is the whole list: the form, the server and the tools use it.
pub const INVEST_TYPES: [(&str, InvestGroup); 12] = [
    ("mutual fund", InvestGroup::Market),
    ("stocks", InvestGroup::Market),
    ("etf", InvestGroup::Market),
    ("bonds", InvestGroup::Market),
    ("crypto", InvestGroup::Market),
    ("fixed deposit", InvestGroup::Deposit),
    ("recurring deposit", InvestGroup::Deposit),
    ("ppf", InvestGroup::Retirement),
    ("epf", InvestGroup::Retirement),
    ("nps", InvestGroup::Retirement),
    ("gold", InvestGroup::Physical),
    ("real estate", InvestGroup::Physical),
];

/// The group of an investment type; `other` (and anything unknown) is [`InvestGroup::Other`].
pub fn invest_group(kind: &str) -> InvestGroup {
    INVEST_TYPES.iter().find(|(k, _)| *k == kind).map(|(_, g)| *g).unwrap_or(InvestGroup::Other)
}

/// Is this a type the app knows? `other` counts.
pub fn is_invest_type(kind: &str) -> bool {
    kind == "other" || INVEST_TYPES.iter().any(|(k, _)| *k == kind)
}

/// The asset kind that goes with a physical investment, for suggesting what to link.
pub fn linkable_asset_kind(invest: &str) -> Option<&'static str> {
    match invest {
        "gold" => Some("gold"),
        "real estate" => Some("property"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Only its owners see it.
    #[default]
    Private,
    /// Everyone in the owner's family sees it, only owners change it.
    Shared,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Visibility::Private => "private",
            Visibility::Shared => "shared",
        }
    }
}

/// Kind-specific fields. Each kind uses its own and ignores the rest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountDetails {
    #[serde(default)]
    pub institution: String,
    #[serde(default)]
    pub last4: String,
    /// credit: the limit
    #[serde(default, with = "money::opt", skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// credit: statement day of month; loan and sip: day of month an instalment leaves
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_day: Option<u32>,
    /// credit: payment due day of month
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_day: Option<u32>,
    /// loan: principal borrowed
    #[serde(default, with = "money::opt", skip_serializing_if = "Option::is_none")]
    pub loan_total: Option<i64>,
    /// loan, deposit, retirement: annual rate in percent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
    /// loan, deposit: months
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenure: Option<u32>,
    /// loan: `YYYY-MM` of the first instalment; deposit: `YYYY-MM` it was opened
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    /// loan: the lender's instalment when known, else computed
    #[serde(default, with = "money::opt", skip_serializing_if = "Option::is_none")]
    pub emi: Option<i64>,
    /// loan: day of month the emi leaves
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emi_day: Option<u32>,
    /// investment: one of [`INVEST_TYPES`], or `other`
    #[serde(default)]
    pub invest_kind: String,
    /// investment: what you have put in (a fixed deposit's lump sum)
    #[serde(default, with = "money::opt", skip_serializing_if = "Option::is_none")]
    pub invested: Option<i64>,
    /// investment: the monthly sip, deposit or contribution
    #[serde(default, with = "money::opt", skip_serializing_if = "Option::is_none")]
    pub sip: Option<i64>,
    /// investment: day of month the sip leaves
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sip_day: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub kind: AccountKind,
    pub owners: Vec<Member>,
    /// More than one owner.
    pub joint: bool,
    pub visibility: Visibility,
    /// What you hold, or what you owe for credit and loan. Never negative for a healthy account; a bank
    /// overdraft shows negative. Derived, never stored: opening balance plus transactions (a loan's
    /// balance comes from its schedule).
    #[serde(with = "money::val")]
    pub balance: i64,
    #[serde(flatten)]
    pub details: AccountDetails,
    /// Schedule figures for a loan with enough details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loan: Option<LoanCalc>,
    /// A fixed or recurring deposit with enough details: worth now and at maturity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deposit: Option<DepositCalc>,
    /// The asset this investment is, when linked. Its value is this account's balance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<LinkedAsset>,
    pub archived: bool,
}

/// The asset behind a linked investment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkedAsset {
    pub id: i64,
    pub name: String,
    pub kind: String,
}

/// The investment an asset is linked to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkedAccount {
    pub id: i64,
    pub name: String,
}

/// `"asset_id": null` unlinks, a number links, leaving it out changes nothing.
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewAccount {
    pub name: String,
    pub kind: AccountKind,
    /// An investment that is one of your assets: its value follows the asset.
    #[serde(default)]
    pub asset_id: Option<i64>,
    /// What you hold now, or owe now for credit. Ignored for loans (they have a schedule).
    #[serde(default, with = "money::opt")]
    pub balance: Option<i64>,
    #[serde(default)]
    pub visibility: Visibility,
    /// Extra owners besides the caller (family members only). Two or more owners make it joint.
    #[serde(default)]
    pub owner_ids: Vec<i64>,
    #[serde(flatten)]
    pub details: AccountDetails,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateAccount {
    pub name: Option<String>,
    /// Link to one of your assets (a number), or unlink (`null`). Investments only.
    #[serde(default, deserialize_with = "double_option")]
    pub asset_id: Option<Option<i64>>,
    pub visibility: Option<Visibility>,
    pub owner_ids: Option<Vec<i64>>,
    pub archived: Option<bool>,
    #[serde(default, with = "money::opt")]
    pub balance: Option<i64>,
    #[serde(flatten)]
    pub details: Option<AccountDetails>,
}

// ---- transactions --------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxKind {
    /// Money out.
    Debit,
    /// Money in.
    Credit,
    /// One leg of a move between two of your accounts.
    Transfer,
}

impl TxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TxKind::Debit => "debit",
            TxKind::Credit => "credit",
            TxKind::Transfer => "transfer",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "debit" => TxKind::Debit,
            "credit" => TxKind::Credit,
            "transfer" => TxKind::Transfer,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: i64,
    pub name: String,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    pub id: i64,
    pub account_id: i64,
    pub kind: TxKind,
    /// Signed from the account's side: money out is negative.
    #[serde(with = "money::val")]
    pub amount: i64,
    /// `YYYY-MM-DD`
    pub date: String,
    pub description: String,
    pub tags: Vec<String>,
    #[serde(default)]
    pub note: String,
    /// The other account of a transfer.
    pub counterpart_id: Option<i64>,
    pub transfer_id: Option<i64>,
    pub created_by: Member,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewTransaction {
    pub account_id: i64,
    /// `debit` or `credit`; use a transfer to move money between accounts.
    pub kind: TxKind,
    /// Positive; the kind decides the direction.
    #[serde(with = "money::val")]
    pub amount: i64,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewTransfer {
    pub from_account_id: i64,
    pub to_account_id: i64,
    #[serde(with = "money::val")]
    pub amount: i64,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateTransaction {
    #[serde(default, with = "money::opt")]
    pub amount: Option<i64>,
    pub date: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub note: Option<String>,
    pub account_id: Option<i64>,
    /// `debit` or `credit`, to turn money out into money in or back. Not for transfers.
    pub kind: Option<TxKind>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TxFilter {
    pub account_id: Option<i64>,
    /// Comma separated account ids.
    pub accounts: Option<String>,
    /// Only transactions someone in this member created.
    pub member_id: Option<i64>,
    /// Comma separated kinds: `debit,credit,transfer`.
    pub kinds: Option<String>,
    /// Comma separated tags; a transaction matches when it has all of them.
    pub tags: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// Substring of description, note or a tag.
    pub q: Option<String>,
    /// Show each transfer once (its outgoing leg) instead of as two rows.
    pub collapse_transfers: Option<bool>,
    /// `date`, `description`, `tag`, `account`, `person`, `amount` (default `date`)
    pub sort: Option<String>,
    /// `asc` or `desc` (default `desc`)
    pub dir: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxPage {
    pub items: Vec<Transaction>,
    /// Total matching the filter, ignoring limit and offset.
    pub total: i64,
    /// Money in and out over everything matching the filter (transfers not counted).
    #[serde(with = "money::val")]
    pub total_in: i64,
    #[serde(with = "money::val")]
    pub total_out: i64,
}

// ---- insights ------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryTotal {
    pub tag: String,
    #[serde(with = "money::val")]
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonthFlow {
    /// `YYYY-MM`
    pub month: String,
    #[serde(with = "money::val")]
    pub income: i64,
    #[serde(with = "money::val")]
    pub spending: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayTotal {
    pub date: String,
    #[serde(with = "money::val")]
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Due {
    /// `YYYY-MM-DD`
    pub date: String,
    pub label: String,
    pub account_id: i64,
    #[serde(with = "money::val")]
    pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Insights {
    #[serde(with = "money::val")]
    pub assets: i64,
    #[serde(with = "money::val")]
    pub owed: i64,
    #[serde(with = "money::val")]
    pub income: i64,
    #[serde(with = "money::val")]
    pub spending: i64,
    /// Spending by tag over the window, biggest first. Investments and transfers are not spending.
    pub categories: Vec<CategoryTotal>,
    /// The last six months.
    pub months: Vec<MonthFlow>,
    /// Daily spending from the monday three weeks before this week's, up to today.
    pub days: Vec<DayTotal>,
    pub dues: Vec<Due>,
    /// What things you own outside your accounts (homes, vehicles, gold) are worth now. Already part of
    /// `assets`; shown apart so a client can draw it as its own segment.
    #[serde(default, with = "money::val")]
    pub things: i64,
    pub loans: Vec<Account>,
    pub investments: Vec<Account>,
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InsightsQuery {
    /// A member id to look at one person; absent is everyone you can see.
    pub member_id: Option<i64>,
    /// Window in days for the category totals, default 30.
    pub days: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ask {
    pub question: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub answer: String,
}

// ---- subscriptions -------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cycle {
    #[default]
    Monthly,
    Yearly,
}

impl Cycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Cycle::Monthly => "monthly",
            Cycle::Yearly => "yearly",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "monthly" => Some(Cycle::Monthly),
            "yearly" => Some(Cycle::Yearly),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subscription {
    pub id: i64,
    pub name: String,
    #[serde(with = "money::val")]
    pub amount: i64,
    pub cycle: Cycle,
    /// `YYYY-MM-DD` of the next renewal; none when it was never scheduled.
    pub next: Option<String>,
    /// The last renewal that was added to transactions.
    pub last: Option<String>,
    /// The account it is paid from.
    pub account_id: i64,
    /// The category its transactions get (they are also tagged `subscription`).
    pub tag: String,
    /// A paused subscription adds nothing.
    pub active: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewSubscription {
    pub name: String,
    #[serde(with = "money::val")]
    pub amount: i64,
    #[serde(default)]
    pub cycle: Cycle,
    #[serde(default)]
    pub next: Option<String>,
    pub account_id: i64,
    #[serde(default)]
    pub tag: String,
    #[serde(default = "yes")]
    pub active: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateSubscription {
    pub name: Option<String>,
    #[serde(default, with = "money::opt")]
    pub amount: Option<i64>,
    pub cycle: Option<Cycle>,
    /// A date, or an empty string to clear it.
    pub next: Option<String>,
    pub account_id: Option<i64>,
    pub tag: Option<String>,
    pub active: Option<bool>,
}

// ---- assets --------------------------------------------------------------------------------------------

pub const ASSET_KINDS: [&str; 5] = ["property", "vehicle", "gold", "electronics", "other"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: i64,
    pub name: String,
    /// One of [`ASSET_KINDS`].
    pub kind: String,
    /// `YYYY-MM`, or empty.
    pub bought: String,
    /// What was paid for it, zero when unknown.
    #[serde(with = "money::val")]
    pub cost: i64,
    /// What it is worth now.
    #[serde(with = "money::val")]
    pub value: i64,
    #[serde(default)]
    pub note: String,
    /// The investment this is linked to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<LinkedAccount>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewAsset {
    pub name: String,
    #[serde(default = "other_kind")]
    pub kind: String,
    #[serde(default)]
    pub bought: String,
    #[serde(default, with = "money::opt")]
    pub cost: Option<i64>,
    #[serde(with = "money::val")]
    pub value: i64,
    #[serde(default)]
    pub note: String,
}

fn other_kind() -> String {
    "other".into()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateAsset {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub bought: Option<String>,
    #[serde(default, with = "money::opt")]
    pub cost: Option<i64>,
    #[serde(default, with = "money::opt")]
    pub value: Option<i64>,
    pub note: Option<String>,
}

// ---- notifications, connectors -------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub id: i64,
    pub title: String,
    pub body: String,
    /// Where it leads in the app: `transactions`, `accounts/3`, `insights`, `settings/family`, or empty.
    pub link: String,
    pub created_at: String,
    pub read: bool,
}

pub const SCOPES: [&str; 4] = ["read", "transactions", "add", "edit"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Connector {
    pub id: i64,
    pub name: String,
    /// What the token may do: `read` accounts and insights, read `transactions`, `add` them, `edit` or
    /// delete them.
    pub scopes: Vec<String>,
    /// Last four characters of the token, to tell tokens apart.
    pub tail: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateConnector {
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewConnector {
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedConnector {
    /// Shown once. Only its hash is stored.
    pub token: String,
    pub connector: Connector,
}

//! The command line. It talks to the database directly, so it works with no server running; `--as` says
//! whose data it is (a person, with full rights). Everything it does goes through the same `tracer-core`
//! functions as the REST API and MCP tools.

use clap::{Args, Parser, Subcommand};
use tracer_core::api::money::{format_minor, group_digits, parse_minor};
use tracer_core::api::*;
use tracer_core::{Caller, Error, Store};

#[derive(Parser)]
#[command(name = "tracer", version, about = "tracer/fin: a household money tracker (CLI, REST and MCP in one binary)")]
pub struct Cli {
    /// Postgres database. The default is the one `docker compose up -d` starts.
    #[arg(long, global = true, env = "TRACER_DB", default_value = tracer_core::DEFAULT_DB, hide_env_values = true)]
    pub db: String,
    /// Act as this person (their email). Not needed when there is only one user.
    #[arg(long = "as", global = true, env = "TRACER_USER")]
    pub who: Option<String>,
    /// Print JSON instead of a table.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Run the REST API, the MCP endpoint (POST /mcp) and the web app.
    Serve {
        #[arg(long, env = "TRACER_LISTEN", default_value = "127.0.0.1:3000")]
        listen: String,
        /// The built web app.
        #[arg(long, env = "TRACER_UI_DIR", default_value = "ui/dist")]
        ui_dir: String,
    },
    /// Run an MCP server on stdio, for Claude Desktop and other local clients.
    Mcp,
    /// People.
    #[command(subcommand)]
    User(UserCmd),
    /// API tokens for Claude, scripts and other tools.
    #[command(subcommand)]
    Token(TokenCmd),
    /// Families.
    #[command(subcommand)]
    Family(FamilyCmd),
    /// List accounts.
    Accounts {
        #[arg(long)]
        all: bool,
    },
    /// Account commands.
    #[command(subcommand)]
    Account(AccountCmd),
    /// Transactions.
    #[command(subcommand)]
    Tx(TxCmd),
    /// Subscriptions: standing charges that become transactions on their renewal date.
    #[command(subcommand, name = "sub")]
    Sub(SubCmd),
    /// Things you own outside your accounts: a home, a vehicle, gold.
    #[command(subcommand)]
    Asset(AssetCmd),
    /// Net worth, spending, what is due.
    Summary {
        #[arg(long, default_value_t = 30)]
        days: u32,
    },
    /// Ask a question about your money.
    Ask { question: Vec<String> },
}

#[derive(Subcommand)]
pub enum UserCmd {
    /// Create a person. The password comes from --password or TRACER_PASSWORD.
    Add {
        name: String,
        email: String,
        #[arg(long, env = "TRACER_PASSWORD", hide_env_values = true)]
        password: String,
    },
    /// Set a password (how a forgotten one is reset). Signs the person out everywhere.
    Passwd {
        email: String,
        #[arg(long, env = "TRACER_PASSWORD", hide_env_values = true)]
        password: String,
    },
}

#[derive(Subcommand)]
pub enum TokenCmd {
    /// Create an API token. Shown once.
    Create {
        name: String,
        /// read, transactions, add, edit
        #[arg(long, value_delimiter = ',', default_value = "read,transactions")]
        scopes: Vec<String>,
    },
    List,
    Revoke { id: i64 },
}

#[derive(Subcommand)]
pub enum FamilyCmd {
    Create { name: String },
    /// Make a one-time invite code (owner only).
    Invite,
    Join { code: String },
    Show,
    Leave,
    /// Delete the family (owner only). Everyone keeps what they own.
    Delete,
}

#[derive(Args)]
pub struct NewAcct {
    pub name: String,
    #[arg(long, default_value = "bank", value_parser = ["bank", "credit", "loan", "investment"])]
    pub kind: String,
    /// What you hold now, or owe now for a credit card.
    #[arg(long)]
    pub balance: Option<String>,
    /// Let your family see it.
    #[arg(long)]
    pub shared: bool,
    /// Co-owner emails (family members): makes a bank account joint.
    #[arg(long = "with", value_delimiter = ',')]
    pub with: Vec<String>,
    #[arg(long)]
    pub institution: Option<String>,
    /// credit: limit
    #[arg(long)]
    pub limit: Option<String>,
    /// credit: payment due day of month
    #[arg(long)]
    pub due_day: Option<u32>,
    /// loan: amount borrowed
    #[arg(long)]
    pub total: Option<String>,
    /// loan: annual rate, percent
    #[arg(long)]
    pub rate: Option<f64>,
    /// loan: months
    #[arg(long)]
    pub tenure: Option<u32>,
    /// loan: first instalment month, YYYY-MM
    #[arg(long)]
    pub start: Option<String>,
    /// loan: day of month the emi leaves
    #[arg(long)]
    pub emi_day: Option<u32>,
    /// investment: amount put in
    #[arg(long)]
    pub invested: Option<String>,
}

#[derive(Subcommand)]
pub enum AccountCmd {
    Add(NewAcct),
    /// Set the current balance of an account (history is kept).
    Set { account: String, balance: String },
    Archive { account: String },
}

#[derive(Subcommand)]
pub enum TxCmd {
    /// Add money out (default) or in.
    Add {
        account: String,
        amount: String,
        description: Vec<String>,
        /// Money in instead of out.
        #[arg(long = "in")]
        credit: bool,
        #[arg(long, value_delimiter = ',')]
        tag: Vec<String>,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Move money between two accounts.
    Transfer {
        from: String,
        to: String,
        amount: String,
        #[arg(long)]
        date: Option<String>,
    },
    List {
        #[arg(short, long)]
        q: Option<String>,
        #[arg(long)]
        account: Option<String>,
        #[arg(long, value_delimiter = ',')]
        tag: Vec<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    Rm { id: i64 },
}

#[derive(Subcommand)]
pub enum SubCmd {
    /// List subscriptions.
    List,
    /// Add one, paid from an account you own.
    Add {
        name: String,
        amount: String,
        account: String,
        /// monthly or yearly
        #[arg(long, default_value = "monthly")]
        cycle: String,
        /// Next renewal, YYYY-MM-DD. A past date is added to transactions at once.
        #[arg(long)]
        next: Option<String>,
        /// The category its transactions get.
        #[arg(long)]
        tag: Option<String>,
    },
    Pause { id: i64 },
    Resume { id: i64 },
    #[command(name = "rm")]
    Delete { id: i64 },
}

#[derive(Subcommand)]
pub enum AssetCmd {
    /// List assets.
    List,
    Add {
        name: String,
        /// What it is worth now.
        value: String,
        /// property, vehicle, gold, electronics or other
        #[arg(long, default_value = "other")]
        kind: String,
        /// Month bought, YYYY-MM
        #[arg(long)]
        bought: Option<String>,
        /// What you paid
        #[arg(long)]
        cost: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Set what an asset is worth now.
    Value { id: i64, value: String },
    #[command(name = "rm")]
    Delete { id: i64 },
}

fn cycle(s: &str) -> Result<Cycle, Error> {
    Cycle::parse(s).ok_or_else(|| Error::bad("cycle is monthly or yearly"))
}

fn money(v: i64) -> String {
    let s = group_digits(v, true);
    if v < 0 { format!("-{s}") } else { s }
}

fn amount(s: &str) -> Result<i64, Error> {
    parse_minor(s).map_err(Error::bad)
}

fn opt_amount(s: &Option<String>) -> Result<Option<i64>, Error> {
    s.as_deref().map(amount).transpose()
}

/// An account by id, exact name, or unique part of a name.
async fn account(s: &Store, c: &Caller, what: &str) -> Result<Account, Error> {
    let all = s.accounts(c, true).await?;
    if let Ok(id) = what.parse::<i64>() {
        if let Some(a) = all.iter().find(|a| a.id == id) {
            return Ok(a.clone());
        }
    }
    let w = what.to_lowercase();
    if let Some(a) = all.iter().find(|a| a.name == w) {
        return Ok(a.clone());
    }
    let hits: Vec<&Account> = all.iter().filter(|a| a.name.contains(&w)).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(Error::bad(format!("no account matches '{what}'. try `tracer accounts`"))),
        many => Err(Error::bad(format!("'{what}' matches {}: {}", many.len(), many.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")))),
    }
}

fn table(headers: &[&str], rows: &[Vec<String>], right: &[usize]) {
    let n = headers.len();
    let mut w: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            w[i] = w[i].max(c.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let parts: Vec<String> = (0..n)
            .map(|i| {
                let pad = w[i] - cells[i].chars().count();
                if right.contains(&i) { format!("{}{}", " ".repeat(pad), cells[i]) } else { format!("{}{}", cells[i], " ".repeat(pad)) }
            })
            .collect();
        println!("{}", parts.join("  ").trim_end());
    };
    line(headers.to_vec());
    for r in rows {
        line(r.iter().map(String::as_str).collect());
    }
}

fn show_json<T: serde::Serialize>(v: &T) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

/// Who the command acts as.
pub async fn caller(s: &Store, who: &Option<String>) -> Result<Caller, Error> {
    if let Ok(token) = std::env::var("TRACER_TOKEN") {
        return s.authenticate(&token).await;
    }
    match who {
        Some(email) => Ok(Caller::full(s.user_by_email(email).await?.id)),
        None => Err(Error::bad("say who you are with --as <email> (or TRACER_USER), or set TRACER_TOKEN")),
    }
}

pub async fn run(cli: Cli, s: Store) -> Result<(), Error> {
    let json = cli.json;
    match cli.cmd {
        Cmd::Serve { .. } | Cmd::Mcp => unreachable!("handled in main"),
        Cmd::User(UserCmd::Add { name, email, password }) => {
            let sess = s.sign_up(SignUp { name, email, password }).await?;
            println!("created {} <{}> (id {})", sess.user.name, sess.user.email, sess.user.id);
            // the sign-up opened a web session: this CLI has no use for it
            s.sign_out(&sess.token).await?;
        }
        Cmd::User(UserCmd::Passwd { email, password }) => {
            let u = s.user_by_email(&email).await?;
            s.set_password(u.id, &password).await?;
            println!("password set for {}; they are signed out everywhere", u.email);
        }
        Cmd::Token(t) => {
            let c = caller(&s, &cli.who).await?;
            match t {
                TokenCmd::Create { name, scopes } => {
                    let made = s.create_connector(&c, NewConnector { name, scopes }).await?;
                    if json {
                        show_json(&serde_json::json!({"token": made.token, "connector": made.connector}));
                    } else {
                        println!("token for '{}' ({}): shown once, store it safely\n{}", made.connector.name, made.connector.scopes.join(","), made.token);
                    }
                }
                TokenCmd::List => {
                    let list = s.connectors(&c).await?;
                    if json {
                        show_json(&list);
                    } else {
                        let rows: Vec<Vec<String>> = list.iter().map(|x| vec![x.id.to_string(), x.name.clone(), x.scopes.join(","), format!("…{}", x.tail), x.last_used_at.clone().unwrap_or("never".into())]).collect();
                        table(&["id", "name", "scopes", "token", "last used"], &rows, &[0]);
                    }
                }
                TokenCmd::Revoke { id } => {
                    s.revoke_connector(&c, id).await?;
                    println!("revoked {id}");
                }
            }
        }
        Cmd::Family(f) => {
            let c = caller(&s, &cli.who).await?;
            match f {
                FamilyCmd::Create { name } => print_family(&s.create_family(&c, NewFamily { name }).await?),
                FamilyCmd::Invite => print_family(&s.generate_invite(&c).await?),
                FamilyCmd::Delete => {
                    s.delete_family(&c).await?;
                    println!("family deleted");
                }
                FamilyCmd::Join { code } => print_family(&s.join_family(&c, JoinFamily { code }).await?),
                FamilyCmd::Show => match s.me(&c).await?.family {
                    Some(f) => print_family(&f),
                    None => println!("not in a family"),
                },
                FamilyCmd::Leave => {
                    s.leave_family(&c).await?;
                    println!("left the family");
                }
            }
        }
        Cmd::Accounts { all } => {
            let c = caller(&s, &cli.who).await?;
            let list = s.accounts(&c, all).await?;
            if json {
                show_json(&list);
            } else {
                let rows: Vec<Vec<String>> = list
                    .iter()
                    .map(|a| {
                        let kind = if a.joint { format!("{} (joint)", a.kind.as_str()) } else { a.kind.as_str().to_string() };
                        let bal = if a.kind.is_liability() { format!("owe {}", money(a.balance)) } else { money(a.balance) };
                        vec![a.id.to_string(), a.name.clone(), kind, a.owners.iter().map(|o| o.name.split(' ').next().unwrap_or("").to_string()).collect::<Vec<_>>().join(", "), bal]
                    })
                    .collect();
                table(&["id", "name", "kind", "owners", "balance"], &rows, &[0, 4]);
            }
        }
        Cmd::Account(a) => {
            let c = caller(&s, &cli.who).await?;
            match a {
                AccountCmd::Add(n) => {
                    let mut owner_ids = Vec::new();
                    for e in &n.with {
                        owner_ids.push(s.user_by_email(e).await?.id);
                    }
                    let created = s
                        .create_account(&c, NewAccount {
                            name: n.name,
                            kind: AccountKind::parse(&n.kind).unwrap(),
                            balance: opt_amount(&n.balance)?,
                            visibility: if n.shared { Visibility::Shared } else { Visibility::Private },
                            owner_ids,
                            details: AccountDetails {
                                institution: n.institution.unwrap_or_default(),
                                limit: opt_amount(&n.limit)?,
                                due_day: n.due_day,
                                loan_total: opt_amount(&n.total)?,
                                rate: n.rate,
                                tenure: n.tenure,
                                start: n.start,
                                emi_day: n.emi_day,
                                invested: opt_amount(&n.invested)?,
                                sip_day: None,
                                ..Default::default()
                            },
                        })
                        .await?;
                    if json {
                        show_json(&created);
                    } else {
                        println!("added account {} '{}', balance {}", created.id, created.name, money(created.balance));
                    }
                }
                AccountCmd::Set { account: what, balance } => {
                    let a = account(&s, &c, &what).await?;
                    let u = s.update_account(&c, a.id, UpdateAccount { balance: Some(amount(&balance)?), ..Default::default() }).await?;
                    println!("{} is now {}", u.name, money(u.balance));
                }
                AccountCmd::Archive { account: what } => {
                    let a = account(&s, &c, &what).await?;
                    s.update_account(&c, a.id, UpdateAccount { archived: Some(true), ..Default::default() }).await?;
                    println!("archived {}", a.name);
                }
            }
        }
        Cmd::Tx(t) => {
            let c = caller(&s, &cli.who).await?;
            match t {
                TxCmd::Add { account: what, amount: amt, description, credit, tag, date, note } => {
                    let a = account(&s, &c, &what).await?;
                    let tx = s
                        .add_transaction(&c, NewTransaction {
                            account_id: a.id,
                            kind: if credit { TxKind::Credit } else { TxKind::Debit },
                            amount: amount(&amt)?,
                            date,
                            description: description.join(" "),
                            tags: tag,
                            note: note.unwrap_or_default(),
                        })
                        .await?;
                    if json {
                        show_json(&tx);
                    } else {
                        println!("#{} {} {} on {} ({})", tx.id, tx.date, money(tx.amount), a.name, tx.description);
                    }
                }
                TxCmd::Transfer { from, to, amount: amt, date } => {
                    let (f, t) = (account(&s, &c, &from).await?, account(&s, &c, &to).await?);
                    let legs = s
                        .transfer(&c, NewTransfer { from_account_id: f.id, to_account_id: t.id, amount: amount(&amt)?, date, description: String::new(), tags: vec![], note: String::new() })
                        .await?;
                    println!("moved {} from {} to {} (#{}, #{})", amt, f.name, t.name, legs[0].id, legs[1].id);
                }
                TxCmd::List { q, account: what, tag, from, to, limit } => {
                    let account_id = match what {
                        Some(w) => Some(account(&s, &c, &w).await?.id),
                        None => None,
                    };
                    let names: std::collections::HashMap<i64, String> = s.accounts(&c, true).await?.into_iter().map(|a| (a.id, a.name)).collect();
                    let page = s
                        .transactions(&c, TxFilter { q, account_id, tags: (!tag.is_empty()).then(|| tag.join(",")), from, to, limit: Some(limit), ..Default::default() })
                        .await?;
                    if json {
                        show_json(&page);
                    } else {
                        let rows: Vec<Vec<String>> = page
                            .items
                            .iter()
                            .map(|t| vec![t.id.to_string(), t.date.clone(), t.description.clone(), t.tags.join(","), names.get(&t.account_id).cloned().unwrap_or_default(), money(t.amount)])
                            .collect();
                        table(&["id", "date", "description", "tags", "account", "amount"], &rows, &[0, 5]);
                        println!("\n{} of {}", page.items.len(), page.total);
                    }
                }
                TxCmd::Rm { id } => {
                    let n = s.delete_transaction(&c, id).await?;
                    println!("deleted {n} {}", if n == 1 { "transaction" } else { "transactions" });
                }
            }
        }
        Cmd::Sub(sc) => {
            let c = caller(&s, &cli.who).await?;
            match sc {
                SubCmd::List => {
                    let list = s.subscriptions(&c).await?;
                    if json {
                        show_json(&list);
                    } else {
                        let names: std::collections::HashMap<i64, String> = s.accounts(&c, true).await?.into_iter().map(|a| (a.id, a.name)).collect();
                        let rows: Vec<Vec<String>> = list
                            .iter()
                            .map(|x| {
                                vec![
                                    x.id.to_string(),
                                    x.name.clone(),
                                    x.cycle.as_str().into(),
                                    x.next.clone().unwrap_or_else(|| "none".into()),
                                    names.get(&x.account_id).cloned().unwrap_or_default(),
                                    if x.active { "active".into() } else { "paused".into() },
                                    money(x.amount),
                                ]
                            })
                            .collect();
                        table(&["id", "name", "billed", "next", "paid from", "status", "amount"], &rows, &[0, 6]);
                    }
                }
                SubCmd::Add { name, amount: amt, account: what, cycle: cy, next, tag } => {
                    let a = account(&s, &c, &what).await?;
                    let x = s
                        .add_subscription(&c, NewSubscription { name, amount: amount(&amt)?, cycle: cycle(&cy)?, next, account_id: a.id, tag: tag.unwrap_or_default(), active: true })
                        .await?;
                    if json {
                        show_json(&x);
                    } else {
                        println!("#{} {} {} {} from {}, next {}", x.id, x.name, money(x.amount), x.cycle.as_str(), a.name, x.next.as_deref().unwrap_or("none"));
                    }
                }
                SubCmd::Pause { id } => {
                    s.update_subscription(&c, id, UpdateSubscription { active: Some(false), ..Default::default() }).await?;
                    println!("paused {id}");
                }
                SubCmd::Resume { id } => {
                    s.update_subscription(&c, id, UpdateSubscription { active: Some(true), ..Default::default() }).await?;
                    println!("resumed {id}");
                }
                SubCmd::Delete { id } => {
                    s.delete_subscription(&c, id).await?;
                    println!("deleted {id}");
                }
            }
        }
        Cmd::Asset(ac) => {
            let c = caller(&s, &cli.who).await?;
            match ac {
                AssetCmd::List => {
                    let list = s.assets(&c).await?;
                    if json {
                        show_json(&list);
                    } else {
                        let rows: Vec<Vec<String>> = list
                            .iter()
                            .map(|x| vec![x.id.to_string(), x.name.clone(), x.kind.clone(), x.bought.clone(), money(x.cost), money(x.value)])
                            .collect();
                        table(&["id", "name", "kind", "bought", "paid", "worth now"], &rows, &[0, 4, 5]);
                    }
                }
                AssetCmd::Add { name, value, kind, bought, cost, note } => {
                    let x = s
                        .add_asset(&c, NewAsset { name, kind, bought: bought.unwrap_or_default(), cost: opt_amount(&cost)?, value: amount(&value)?, note: note.unwrap_or_default() })
                        .await?;
                    if json {
                        show_json(&x);
                    } else {
                        println!("#{} {} ({}) worth {}", x.id, x.name, x.kind, money(x.value));
                    }
                }
                AssetCmd::Value { id, value } => {
                    let x = s.update_asset(&c, id, UpdateAsset { value: Some(amount(&value)?), ..Default::default() }).await?;
                    println!("{} is now worth {}", x.name, money(x.value));
                }
                AssetCmd::Delete { id } => {
                    s.delete_asset(&c, id).await?;
                    println!("deleted {id}");
                }
            }
        }
        Cmd::Summary { days } => {
            let c = caller(&s, &cli.who).await?;
            let i = s.insights(&c, InsightsQuery { member_id: None, days: Some(days) }).await?;
            if json {
                show_json(&i);
            } else {
                let things = if i.things > 0 { format!(" (things you own: {})", money(i.things)) } else { String::new() };
                println!("assets   {}{things}\nowed     {}\nworth    {}\n", money(i.assets), money(i.owed), money(i.assets - i.owed));
                println!("last {days} days: income {}, spending {}", money(i.income), money(i.spending));
                for c in i.categories.iter().take(8) {
                    println!("  {:<14}{:>12}", c.tag, money(c.total));
                }
                if !i.dues.is_empty() {
                    println!("\ndue");
                    for d in &i.dues {
                        println!("  {}  {:<22}{:>12}", d.date, d.label, money(d.amount));
                    }
                }
            }
        }
        Cmd::Ask { question } => {
            let c = caller(&s, &cli.who).await?;
            println!("{}", s.ask(&c, &question.join(" ")).await?);
        }
    }
    let _ = format_minor;
    Ok(())
}

fn print_family(f: &Family) {
    println!("{}\nmembers: {}", f.name, f.members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", "));
    if let Some(code) = &f.invite_code {
        println!("invite code (works once): {code}");
    }
}

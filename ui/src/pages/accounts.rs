use dots_design::prelude::*;
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use tracer_api::money::{format_minor, parse_minor};
use tracer_api::*;

use crate::api;
use crate::fmt;
use crate::icons::*;
use crate::state::AppState;

/// The kind as the screens name it: a bank account with more than one owner is `joint`.
fn kind_key(a: &Account) -> &'static str {
    match (a.kind, a.joint) {
        (AccountKind::Bank, true) => "joint",
        (k, _) => k.as_str(),
    }
}

fn kind_plural(k: &str) -> &'static str {
    match k {
        "joint" => "joint",
        "credit" => "credit",
        "loan" => "loans",
        "investment" => "investments",
        _ => "bank",
    }
}

fn setup_title(k: &str) -> &'static str {
    match k {
        "joint" => "joint account",
        "credit" => "credit card",
        "loan" => "loan",
        "investment" => "investment",
        _ => "bank account",
    }
}

fn vis_label(a: &Account) -> &'static str {
    match (a.joint, a.visibility) {
        (true, _) => "joint",
        (_, Visibility::Shared) => "shared",
        _ => "private",
    }
}

fn owners_line(a: &Account) -> String {
    a.owners.iter().map(|o| o.name.split(' ').next().unwrap_or("").to_string()).collect::<Vec<_>>().join(" and ")
}

fn signed_balance(a: &Account, app: &AppState) -> String {
    format!("{}{}", if a.kind.is_liability() { "−" } else { "" }, app.money(a.balance))
}

// ---- the list ----------------------------------------------------------------------------------------

#[component]
pub fn Accounts() -> impl IntoView {
    let app = expect_context::<AppState>();
    let nav = use_navigate();
    let q = RwSignal::new(String::new());
    let kinds = RwSignal::new(Vec::<String>::new());
    let sort_key = RwSignal::new("");
    let sort_dir = RwSignal::new(SortDirection::Ascending);
    let all = move || app.accounts.get().into_iter().filter(|a| !a.archived).collect::<Vec<_>>();
    let in_family = Memo::new(move |_| app.in_family());
    let kind_order = move || {
        let mut k = vec!["bank"];
        if all().iter().any(|a| kind_key(a) == "joint") {
            k.push("joint");
        }
        k.extend(["credit", "loan", "investment"]);
        k
    };
    let kind_options = Signal::derive(move || {
        let list = all();
        kind_order().into_iter().map(|k| FilterOption::new(k, kind_plural(k)).meta(list.iter().filter(|a| kind_key(a) == k).count().to_string())).collect::<Vec<_>>()
    });
    let rows = move || {
        let needle = q.get().trim().to_lowercase();
        let ks = kinds.get();
        let mut list: Vec<Account> = all().into_iter().filter(|a| (ks.is_empty() || ks.iter().any(|k| k == kind_key(a))) && (needle.is_empty() || a.name.contains(&needle))).collect();
        let key = sort_key.get();
        if !key.is_empty() {
            list.sort_by(|x, y| {
                let o = match key {
                    "name" => x.name.cmp(&y.name),
                    "kind" => kind_key(x).cmp(kind_key(y)),
                    "owner" => owners_line(x).cmp(&owners_line(y)),
                    "vis" => vis_label(x).cmp(vis_label(y)),
                    _ => {
                        let v = |a: &Account| if a.kind.is_liability() { -a.balance } else { a.balance };
                        v(x).cmp(&v(y))
                    }
                };
                if sort_dir.get() == SortDirection::Descending { o.reverse() } else { o }
            });
        }
        list
    };
    let summary = move || {
        let list = all();
        let own: i64 = list.iter().filter(|a| !a.kind.is_liability()).map(|a| a.balance).sum();
        let owe: i64 = list.iter().filter(|a| a.kind.is_liability()).map(|a| a.balance).sum();
        format!("{} {} · own {} · owe {} · click an account to open it", list.len(), if list.len() == 1 { "account" } else { "accounts" }, app.money(own), app.money(owe))
    };
    let th = move |key: &'static str| {
        let dir = Signal::derive(move || if sort_key.get() == key { sort_dir.get() } else { SortDirection::None });
        let on = Callback::new(move |()| {
            if sort_key.get_untracked() == key {
                sort_dir.update(|d| *d = d.next());
            } else {
                sort_key.set(key);
                sort_dir.set(SortDirection::Ascending);
            }
        });
        (dir, on)
    };
    let any_filter = move || !kinds.get().is_empty() || !q.get().trim().is_empty();
    let new_href = move || match kinds.get().as_slice() {
        [one] => format!("/accounts/new?kind={one}"),
        _ => "/accounts/new".to_string(),
    };
    view! {
        <div style="display:flex;flex-wrap:wrap;gap:16px;align-items:flex-start">
            <div style="flex:999 1 560px;min-width:0;display:flex;flex-direction:column;gap:16px">
                <div class="d-row" style="justify-content:space-between;gap:12px">
                    <div style="display:flex;flex-direction:column;gap:4px">
                        <h1 class="heading-xl">"accounts"</h1>
                        <span style="color:var(--text-secondary)">{summary}</span>
                    </div>
                    <a class="d-btn d-btn--primary" href=new_href><Ico d=PLUS/>"add account"</a>
                </div>
                <FilterBar label="filters">
                    <FilterSearch label="search accounts" value=q placeholder="search by name"/>
                    <Filter label="kind" options=kind_options selected=kinds/>
                    {move || any_filter().then(|| view! { <Button variant=ButtonVariant::Ghost on:click=move |_| { kinds.set(Vec::new()); q.set(String::new()); }>"clear all"</Button> })}
                </FilterBar>
                {move || {
                    let list = rows();
                    if list.is_empty() {
                        return view! {
                            <EmptyState title=if any_filter() { "no accounts match" } else { "no accounts yet" } hint=if any_filter() { "add one, or clear the filters." } else { "add the places your money lives: a bank account, a card, a loan, an investment." }>
                                <ButtonLink href="/accounts/new">"add account"</ButtonLink>
                            </EmptyState>
                        }.into_any();
                    }
                    let fam = in_family.get();
                    let cols = [th("name"), th("kind"), th("owner"), th("vis"), th("balance")];
                    let nav = nav.clone();
                    view! {
                        <Table label="accounts. select a row to open it" size=Size::Lg>
                            <thead><tr>
                                <Th sort=cols[0].0 on_sort=cols[0].1>"name"</Th>
                                <Th sort=cols[1].0 on_sort=cols[1].1>"kind"</Th>
                                {fam.then(|| view! { <Th sort=cols[2].0 on_sort=cols[2].1>"owner"</Th> })}
                                {fam.then(|| view! { <Th sort=cols[3].0 on_sort=cols[3].1>"who sees it"</Th> })}
                                <Th num=true sort=cols[4].0 on_sort=cols[4].1>"balance"</Th>
                            </tr></thead>
                            <tbody>
                                {list.into_iter().map(|a| {
                                    let nav = nav.clone();
                                    let id = a.id;
                                    let (name, kind, owner, vis, amount) = (a.name.clone(), kind_key(&a), owners_line(&a), vis_label(&a), signed_balance(&a, &app));
                                    view! {
                                        <Tr selectable=true on_select=move |()| nav(&format!("/accounts/{id}"), Default::default())>
                                            <td style="color:var(--text-strong)">{name}</td>
                                            <td><span class="d-badge">{kind}</span></td>
                                            {fam.then(|| view! { <td>{owner}</td> })}
                                            {fam.then(|| view! { <td>{vis}</td> })}
                                            <td class="d-num" style="color:var(--text-strong)">{amount}</td>
                                        </Tr>
                                    }
                                }).collect_view()}
                            </tbody>
                        </Table>
                    }.into_any()
                }}
            </div>
        </div>
    }
}

// ---- the form both screens share ---------------------------------------------------------------------

/// Every field an account can have, as text while it is being typed.
#[derive(Clone, Copy)]
struct Form {
    name: RwSignal<String>,
    institution: RwSignal<String>,
    last4: RwSignal<String>,
    balance: RwSignal<String>,
    limit: RwSignal<String>,
    statement_day: RwSignal<String>,
    due_day: RwSignal<String>,
    loan_total: RwSignal<String>,
    rate: RwSignal<String>,
    tenure: RwSignal<String>,
    start: RwSignal<String>,
    emi: RwSignal<String>,
    emi_day: RwSignal<String>,
    invest_kind: RwSignal<String>,
    invested: RwSignal<String>,
    sip: RwSignal<String>,
    sip_day: RwSignal<String>,
    visibility: RwSignal<String>,
    /// Family members who own it with you. Bank accounts only: any co-owner makes it a joint account.
    co_owners: RwSignal<Vec<i64>>,
}

struct Field {
    id: &'static str,
    label: String,
    kind: &'static str,
    hint: &'static str,
    optional: bool,
    value: RwSignal<String>,
}

impl Form {
    fn new(a: Option<&Account>, me: i64) -> Self {
        let d = a.map(|a| a.details.clone()).unwrap_or_default();
        let s = |v: String| RwSignal::new(v);
        let m = |v: Option<i64>| RwSignal::new(v.map(fmt::plain).unwrap_or_default());
        let n = |v: Option<u32>| RwSignal::new(v.map(|v| v.to_string()).unwrap_or_default());
        Form {
            name: s(a.map(|a| a.name.clone()).unwrap_or_default()),
            institution: s(d.institution),
            last4: s(d.last4),
            balance: s(a.filter(|a| a.kind != AccountKind::Loan).map(|a| fmt::plain(a.balance)).unwrap_or_default()),
            limit: m(d.limit),
            statement_day: n(d.statement_day),
            due_day: n(d.due_day),
            loan_total: m(d.loan_total),
            rate: s(d.rate.map(|r| r.to_string()).unwrap_or_default()),
            tenure: n(d.tenure),
            start: s(d.start.unwrap_or_default()),
            emi: m(d.emi),
            emi_day: n(d.emi_day),
            invest_kind: s(d.invest_kind),
            invested: m(d.invested),
            sip: m(d.sip),
            sip_day: n(d.sip_day),
            visibility: s(a.map(|a| a.visibility.as_str().to_string()).unwrap_or_else(|| "private".into())),
            co_owners: RwSignal::new(a.map(|a| a.owners.iter().map(|o| o.id).filter(|o| *o != me).collect()).unwrap_or_default()),
        }
    }

    /// The fields this kind asks for, in the order people think of them.
    fn fields(&self, kind: &str, sym: &str) -> Vec<Field> {
        let f = |id, label: &str, kind, hint, optional, value| Field { id, label: label.replace('₹', sym), kind, hint, optional, value };
        match kind {
            "credit" => vec![
                f("inst", "issuer", "text", "", true, self.institution),
                f("limit", "credit limit (₹)", "text", "", false, self.limit),
                f("balance", "outstanding now (₹)", "text", "what you owe today", false, self.balance),
                f("stmt", "statement day", "number", "day of the month the bill is made", false, self.statement_day),
                f("due", "payment due day", "number", "day of the month, like 18", false, self.due_day),
            ],
            "loan" => vec![
                f("inst", "lender", "text", "", true, self.institution),
                f("total", "amount borrowed (₹)", "text", "", false, self.loan_total),
                f("rate", "interest rate (% a year)", "text", "", false, self.rate),
                f("tenure", "tenure (months)", "number", "like 180 for 15 years", false, self.tenure),
                f("start", "first emi month", "month", "", false, self.start),
                f("emiday", "emi day", "number", "day of the month, like 5", false, self.emi_day),
                f("emi", "emi (₹)", "text", "leave empty to calculate it", true, self.emi),
            ],
            "investment" => vec![
                f("invkind", "kind", "text", "mutual fund, fixed deposit, ppf, stocks", false, self.invest_kind),
                f("invested", "amount invested (₹)", "text", "", false, self.invested),
                f("balance", "current value (₹)", "text", "", false, self.balance),
                f("sip", "monthly sip (₹)", "text", "if you add to it every month", true, self.sip),
                f("sipday", "sip day", "number", "day of the month, like 2", true, self.sip_day),
            ],
            _ => vec![
                f("inst", "institution", "text", "", true, self.institution),
                f("last4", "number ends", "text", "last 4 digits, to tell accounts apart", true, self.last4),
                f("balance", "current balance (₹)", "text", "", false, self.balance),
            ],
        }
    }

    fn loan(&self) -> Option<tracer_api::loan::LoanCalc> {
        let total = parse_minor(&self.loan_total.get()).ok()?;
        let now = js_sys::Date::new_0();
        tracer_api::loan::compute(total, self.rate.get().trim().parse().unwrap_or(0.0), self.tenure.get().trim().parse().ok()?, &self.start.get(), parse_minor(&self.emi.get()).ok().filter(|e| *e > 0), (now.get_full_year() as i32, now.get_month() + 1))
    }

    /// What the details add up to, as label and value pairs. Updates as you type.
    fn summary(&self, kind: &str, app: &AppState) -> Vec<(String, String)> {
        let num = |s: RwSignal<String>| parse_minor(&s.get()).unwrap_or(0).abs();
        let row = |l: &str, v: String| (l.to_string(), v);
        match kind {
            "loan" => match self.loan() {
                Some(l) => vec![
                    row("emi", format!("{} a month", app.money(l.emi))),
                    row("months paid", format!("{} of {}", l.paid, l.tenure)),
                    row("months left", l.left.to_string()),
                    row("last emi", fmt::month_year(&l.end)),
                    row("principal left", app.money(l.balance)),
                    row("interest over the loan", app.money(l.interest)),
                ],
                None => vec![row("emi", "needs amount, tenure and first month".into())],
            },
            "credit" => {
                let (lim, out) = (num(self.limit), num(self.balance));
                let due = self.due_day.get().trim().parse::<u32>().ok().filter(|d| (1..=31).contains(d));
                vec![
                    row("available", if lim > 0 { app.money((lim - out).max(0)) } else { "needs a limit".into() }),
                    row("used", if lim > 0 { fmt::pct(out, lim) } else { "needs a limit".into() }),
                    row("next payment", due.map(|d| format!("on the {}", fmt::ordinal(d))).unwrap_or_else(|| "not set".into())),
                ]
            }
            "investment" => {
                let (inv, cur) = (num(self.invested), num(self.balance));
                let sip = num(self.sip);
                let day = self.sip_day.get().trim().parse::<u32>().ok().filter(|d| (1..=31).contains(d));
                vec![
                    row("gain so far", if inv > 0 { format!("{} {}", if cur >= inv { "↑" } else { "↓" }, app.money((cur - inv).abs())) } else { "needs amount invested".into() }),
                    row("monthly sip", if sip > 0 { format!("{}{}", app.money(sip), day.map(|d| format!(" on the {}", fmt::ordinal(d))).unwrap_or_default()) } else { "none".into() }),
                ]
            }
            _ => vec![row("opening balance", app.money(num(self.balance)))],
        }
        .into_iter()
        .chain(std::iter::once(row("who sees it", self.seen_by(app))))
        .collect()
    }

    /// `you and vikram`, `the family` or `only you`.
    fn seen_by(&self, app: &AppState) -> String {
        let with = self.co_owners.get();
        if !with.is_empty() {
            let names: Vec<String> = app.members().into_iter().filter(|m| with.contains(&m.id)).map(|m| m.name.split(' ').next().unwrap_or("").to_string()).collect();
            return format!("you and {}", names.join(" and "));
        }
        if self.visibility.get() == "shared" { "the family".into() } else { "only you".into() }
    }

    fn note(kind: &str) -> &'static str {
        match kind {
            "loan" => "insights will track months left and the end date. record each emi as a transfer to this loan.",
            "credit" => "spend on the card as normal transactions. settle the bill as a transfer from a bank account to this card.",
            "investment" => "record each sip or top-up as a transfer from a bank account to this investment.",
            _ => "money in and out of this account is recorded on the transactions screen.",
        }
    }

    /// The request body: only what this kind uses, amounts as decimal strings.
    fn body(&self, kind: &str, set_owners: bool) -> Result<serde_json::Map<String, serde_json::Value>, String> {
        let mut o = serde_json::Map::new();
        if set_owners {
            if kind == "joint" && self.co_owners.get_untracked().is_empty() {
                return Err("a joint account needs at least one other owner: tick who it is joint with".into());
            }
            o.insert("owner_ids".into(), self.co_owners.get_untracked().into());
        }
        let name = self.name.get_untracked().trim().to_lowercase();
        if name.is_empty() {
            return Err("give the account a name".into());
        }
        o.insert("name".into(), name.into());
        let money = |o: &mut serde_json::Map<String, serde_json::Value>, key: &str, s: RwSignal<String>, what: &str| -> Result<(), String> {
            let v = s.get_untracked();
            if v.trim().is_empty() {
                return Ok(());
            }
            let m = parse_minor(&v).map_err(|_| format!("{what} is not a number"))?;
            o.insert(key.into(), format_minor(m.abs()).into());
            Ok(())
        };
        let day = |o: &mut serde_json::Map<String, serde_json::Value>, key: &str, s: RwSignal<String>, what: &str| -> Result<(), String> {
            let v = s.get_untracked();
            if v.trim().is_empty() {
                return Ok(());
            }
            match v.trim().parse::<u32>() {
                Ok(d) if (1..=31).contains(&d) => {
                    o.insert(key.into(), d.into());
                    Ok(())
                }
                _ => Err(format!("{what} must be a day of the month, 1 to 31")),
            }
        };
        o.insert("institution".into(), self.institution.get_untracked().trim().into());
        match kind {
            "credit" => {
                money(&mut o, "limit", self.limit, "the limit")?;
                money(&mut o, "balance", self.balance, "the outstanding amount")?;
                day(&mut o, "statement_day", self.statement_day, "statement day")?;
                day(&mut o, "due_day", self.due_day, "payment due day")?;
            }
            "loan" => {
                money(&mut o, "loan_total", self.loan_total, "the amount borrowed")?;
                if let Ok(r) = self.rate.get_untracked().trim().parse::<f64>() {
                    o.insert("rate".into(), r.into());
                }
                if let Ok(t) = self.tenure.get_untracked().trim().parse::<u32>() {
                    o.insert("tenure".into(), t.into());
                }
                let start: String = self.start.get_untracked().trim().chars().take(7).collect();
                if !start.is_empty() {
                    o.insert("start".into(), start.into());
                }
                money(&mut o, "emi", self.emi, "the emi")?;
                day(&mut o, "emi_day", self.emi_day, "emi day")?;
            }
            "investment" => {
                o.insert("invest_kind".into(), self.invest_kind.get_untracked().trim().into());
                money(&mut o, "invested", self.invested, "the amount invested")?;
                money(&mut o, "balance", self.balance, "the current value")?;
                money(&mut o, "sip", self.sip, "the sip")?;
                day(&mut o, "sip_day", self.sip_day, "sip day")?;
            }
            _ => {
                o.insert("last4".into(), self.last4.get_untracked().trim().into());
                money(&mut o, "balance", self.balance, "the balance")?;
            }
        }
        Ok(o)
    }
}

#[component]
fn Fields(form: Form, kind: String, #[prop(into)] name_hint: String, id: &'static str) -> impl IntoView {
    let app = expect_context::<AppState>();
    let sym = fmt::symbol(&app.currency());
    let name_id = format!("{id}-name");
    let name_hint_id = format!("{id}-name-h");
    let has_hint = !name_hint.is_empty();
    view! {
        <div class="d-field">
            <label class="d-label" for=name_id.clone()>"name"</label>
            <input class="d-input" id=name_id type="text" aria-describedby=has_hint.then(|| name_hint_id.clone()) prop:value=move || form.name.get() on:input=move |e| form.name.set(event_target_value(&e))/>
            {has_hint.then(|| view! { <span class="d-hint" id=name_hint_id.clone()>{name_hint}</span> })}
        </div>
        {form.fields(&kind, sym).into_iter().map(|f| {
            let fid = format!("{id}-{}", f.id);
            let hid = format!("{fid}-h");
            let v = f.value;
            let inputmode = if f.label.contains(sym) || f.kind == "number" { Some("decimal") } else { None };
            view! {
                <div class="d-field">
                    <label class="d-label" for=fid.clone()>{f.label}{f.optional.then(|| view! { " "<span>"(optional)"</span> })}</label>
                    <input class="d-input" id=fid type=if f.kind == "month" { "month" } else { "text" } inputmode=inputmode aria-describedby=(!f.hint.is_empty()).then(|| hid.clone())
                        prop:value=move || v.get() on:input=move |e| v.set(event_target_value(&e))/>
                    {(!f.hint.is_empty()).then(|| view! { <span class="d-hint" id=hid.clone()>{f.hint}</span> })}
                </div>
            }
        }).collect_view()}
    }
}

/// "joint with": tick the family members who own the account with you.
#[component]
fn Owners(form: Form) -> impl IntoView {
    let app = expect_context::<AppState>();
    let others = move || app.members().into_iter().filter(|m| m.id != app.my_id()).collect::<Vec<_>>();
    view! {
        <fieldset class="d-field" style="margin:0;padding:0;border:0;min-width:0">
            <legend class="d-label" style="padding:0">"joint with"</legend>
            {move || others().into_iter().map(|m| {
                let id = m.id;
                view! {
                    <label class="d-choice">
                        <input type="checkbox" class="d-check" prop:checked=move || form.co_owners.get().contains(&id)
                            on:change=move |e| { let on = event_target_checked(&e); form.co_owners.update(|v| { v.retain(|x| *x != id); if on { v.push(id); } }); }/>
                        <span>{m.name.clone()}</span>
                    </label>
                }
            }).collect_view()}
            <span class="d-hint">"everyone ticked owns it with you: they see it and can add to it."</span>
        </fieldset>
    }
}

#[component]
fn SummaryList(#[prop(into)] rows: Signal<Vec<(String, String)>>) -> impl IntoView {
    view! {
        <ul class="d-list d-list--lg">
            {move || rows.get().into_iter().map(|(l, v)| view! {
                <li><span class="d-list__row">{l}<span class="d-list__meta" style="color:var(--text-strong)">{v}</span></span></li>
            }).collect_view()}
        </ul>
    }
}

// ---- add an account: kind, details, review ------------------------------------------------------------

#[component]
pub fn NewAccount() -> impl IntoView {
    let app = expect_context::<AppState>();
    let nav = use_navigate();
    let query = use_query_map();
    let start_kind = query.with_untracked(|q| q.get("kind")).filter(|k| ["bank", "joint", "credit", "loan", "investment"].contains(&k.as_str())).unwrap_or_else(|| "bank".into());
    let kind = RwSignal::new(start_kind);
    let step = RwSignal::new(1u8);
    let form = Form::new(None, app.my_id());
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let others = move || app.members().into_iter().filter(|m| m.id != app.my_id()).collect::<Vec<_>>();
    let has_others = Memo::new(move |_| !others().is_empty());
    // picking "joint" ticks everyone; picking another kind starts with just you
    Effect::new(move |_| {
        let joint = kind.get() == "joint";
        form.co_owners.set(if joint { others().into_iter().map(|m| m.id).collect() } else { Vec::new() });
    });
    let kinds = move || {
        let mut k = vec![("bank", "bank", "asset", "savings or current account".to_string()), ("credit", "credit card", "owed", "limit, bill date, due date".to_string()), ("loan", "loan", "owed", "amount, rate, tenure, emi".to_string()), ("investment", "investment", "asset", "funds, deposits, sip".to_string())];
        let o = others();
        k.push(("joint", "joint", "asset", if o.is_empty() { "owned with someone in your family".to_string() } else { format!("shared with {}", o.iter().map(|m| m.name.split(' ').next().unwrap_or("").to_string()).collect::<Vec<_>>().join(" and ")) }));
        k
    };
    let lead = move || ["pick the kind of account. each kind asks for different details.", "fill in what this kind needs. optional fields can wait.", "check it over, then add it."][step.get() as usize - 1];
    let summary = Signal::derive(move || form.summary(&kind.get(), &app));
    let review = Signal::derive(move || {
        let k = kind.get();
        let mut rows: Vec<(String, String)> = form.fields(&k, fmt::symbol(&app.currency())).into_iter().filter(|f| !f.value.get().trim().is_empty()).map(|f| (f.label, f.value.get())).collect();
        rows.extend(form.summary(&k, &app));
        rows
    });
    let save = {
        let nav = nav.clone();
        move || {
            let k = kind.get_untracked();
            let mut body = match form.body(&k, k == "joint") {
                Ok(b) => b,
                Err(e) => {
                    step.set(2);
                    return err.set(Some(e));
                }
            };
            body.insert("kind".into(), if k == "joint" { "bank" } else { k.as_str() }.into());
            busy.set(true);
            let nav = nav.clone();
            leptos::task::spawn_local(async move {
                let r = api::post::<Account>("/accounts", &serde_json::Value::Object(body)).await;
                busy.set(false);
                match r {
                    Ok(_) => {
                        app.reload();
                        nav("/accounts", Default::default());
                    }
                    Err(e) if e.status == 400 || e.status == 403 => {
                        step.set(2);
                        err.set(Some(e.message));
                    }
                    Err(e) => app.fail(&e),
                }
            });
        }
    };
    let next = move || match step.get_untracked() {
        1 => step.set(2),
        2 if kind.get_untracked() == "joint" && !has_others.get_untracked() => {}
        2 => match form.body(&kind.get_untracked(), kind.get_untracked() == "joint") {
            Ok(_) => {
                err.set(None);
                step.set(3);
            }
            Err(e) => err.set(Some(e)),
        },
        _ => save(),
    };
    let next2 = next.clone();
    let owner = move || app.me.with(|m| m.as_ref().map(|m| m.user.name.clone()).unwrap_or_default());
    let name_hint = |k: &str| match k {
        "joint" => "like household joint",
        "credit" => "like travel card",
        "loan" => "like home loan",
        "investment" => "like index fund",
        _ => "like salary account",
    };
    view! {
        <form on:submit=move |e| { e.prevent_default(); next() } style="max-width:960px;margin-inline:auto;display:flex;flex-direction:column;gap:20px">
            <div class="d-row" style="justify-content:space-between;gap:12px">
                <div style="display:flex;flex-direction:column;gap:4px">
                    <h1 class="heading-xl">"add account"</h1>
                    <span style="color:var(--text-secondary)">{lead}</span>
                </div>
                <a class="d-btn d-btn--ghost" href="/accounts">"cancel"</a>
            </div>
            <ol class="aj-steps" aria-label="steps">
                {["kind", "details", "review"].into_iter().enumerate().map(|(i, l)| {
                    let n = i as u8 + 1;
                    view! { <li data-s={move || if step.get() == n { "on" } else if step.get() > n { "done" } else { "todo" }} aria-current=move || (step.get() == n).then_some("step")><b aria-hidden="true">{n.to_string()}</b><span>{l}</span></li> }
                }).collect_view()}
            </ol>
            {move || match step.get() {
                1 => view! {
                    <fieldset style="margin:0;padding:0;border:0;display:grid;grid-template-columns:repeat(auto-fit,minmax(min(260px,100%),1fr));gap:12px">
                        <legend class="d-sr">"kind of account"</legend>
                        {kinds().into_iter().map(|(k, label, side, note)| view! {
                            <button type="button" class="d-card" aria-pressed=move || (kind.get() == k).to_string() on:click=move |_| kind.set(k.into()) style="cursor:pointer;font:inherit;text-align:left">
                                <span class="d-card__head"><span class="d-card__title">{label}</span><span class="d-card__meta">{side}</span></span>
                                <span class="d-card__body" style="display:block">{note}</span>
                            </button>
                        }).collect_view()}
                    </fieldset>
                }.into_any(),
                2 if kind.get() == "joint" && !has_others.get() => view! {
                    <section class="d-card" aria-labelledby="jn-t" style="max-width:640px">
                        <header class="d-card__head"><h2 class="d-card__title" id="jn-t">"joint account"</h2><span class="d-card__meta">"needs a family"</span></header>
                        <div class="d-card__body" style="display:flex;flex-direction:column;gap:12px;align-items:flex-start">
                            <p style="max-width:64ch">"a joint account is owned by you and someone in your family: both of you see it and add to it. there is nobody to share it with yet."</p>
                            <p style="max-width:64ch">"create a family and send them an invite code. once they join, come back here."</p>
                            <a class="d-btn d-btn--secondary" href="/settings/family">"go to family settings"</a>
                        </div>
                    </section>
                }.into_any(),
                2 => {
                    let k = kind.get();
                    view! {
                        <div style="display:flex;flex-wrap:wrap;gap:16px;align-items:flex-start">
                            <section class="d-card" aria-labelledby="st-t" style="flex:3 1 420px">
                                <header class="d-card__head"><h2 class="d-card__title" id="st-t">{setup_title(&k)}</h2><span class="d-card__meta">{move || format!("owner: {}", owner())}</span></header>
                                <div class="d-card__body" style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(220px,100%),1fr));gap:16px;align-items:start">
                                    <Fields form=form kind=k.clone() name_hint=name_hint(&k) id="st"/>
                                    {(k == "joint").then(|| view! { <Owners form=form/> })}
                                </div>
                                {move || err.get().map(|m| view! { <div class="d-card__body" style="padding-top:0"><span class="d-error" role="alert">{format!("error: {m}")}</span></div> })}
                            </section>
                            <section class="d-card" aria-labelledby="st-s" style="flex:2 1 280px">
                                <header class="d-card__head"><h2 class="d-card__title" id="st-s">"what this sets up"</h2><span class="d-card__meta">"updates as you type"</span></header>
                                <div class="d-card__body d-card__body--flush"><SummaryList rows=summary/></div>
                            </section>
                        </div>
                    }.into_any()
                }
                _ => {
                    let k = kind.get();
                    view! {
                        <section class="d-card" aria-labelledby="rv-t" style="max-width:640px">
                            <header class="d-card__head"><h2 class="d-card__title" id="rv-t">{move || form.name.get().trim().to_lowercase()}</h2><span class="d-card__meta">{setup_title(&k)}</span></header>
                            <div class="d-card__body d-card__body--flush"><SummaryList rows=review/></div>
                            <div class="d-card__body" style="border-top:1px solid var(--border-default);color:var(--text-secondary)">{Form::note(&k)}</div>
                        </section>
                    }.into_any()
                }
            }}
            <div class="d-row" style="justify-content:space-between;gap:12px">
                <span>{move || { step.get() > 1 }.then(|| view! { <button type="button" class="d-btn d-btn--secondary" on:click=move |_| step.update(|s| *s -= 1)>"back"</button> })}</span>
                <button type="button" class="d-btn d-btn--primary" aria-busy=move || busy.get().then_some("true")
                    aria-disabled=move || (step.get() == 2 && kind.get() == "joint" && !has_others.get()).then_some("true")
                    on:click={ let next = next2.clone(); move |_| next() }>
                    {move || match step.get() { 1 => "continue", 2 => "review", _ => "add account" }}
                </button>
            </div>
        </form>
    }
}

// ---- one account: its summary, and its details to change -------------------------------------------

#[component]
pub fn AccountPage() -> impl IntoView {
    let app = expect_context::<AppState>();
    let params = use_params_map();
    let id = Memo::new(move |_| params.with(|p| p.get("id")).and_then(|i| i.parse::<i64>().ok()).unwrap_or(0));
    let account = Memo::new(move |_| app.accounts.with(|a| a.iter().find(|a| a.id == id.get()).cloned()));
    let loaded = Memo::new(move |_| app.me.with(|m| m.is_some()));
    // lives here so it survives the refresh that follows a save
    let saved = RwSignal::new(false);
    Effect::new(move |_| {
        id.track();
        saved.set(false);
    });
    view! {
        {move || match (account.get(), loaded.get()) {
            (Some(a), _) => view! { <AccountView a=a saved=saved/> }.into_any(),
            (None, false) => view! { <EmptyLoading title="loading the account"/> }.into_any(),
            (None, true) => view! {
                <EmptyState title="no such account" hint="it may have been deleted, or it is not shared with you.">
                    <ButtonLink href="/accounts">"all accounts"</ButtonLink>
                </EmptyState>
            }.into_any(),
        }}
    }
}

#[component]
fn AccountView(a: Account, saved: RwSignal<bool>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let nav = use_navigate();
    let id = a.id;
    let kind = kind_key(&a);
    let form = Form::new(Some(&a), app.my_id());
    let err = RwSignal::new(None::<String>);
    let confirm = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let owned = a.owners.iter().any(|o| o.id == app.my_id());
    // only a bank account can be joint
    let has_others = owned && app.has_family() && a.kind == AccountKind::Bank;
    let is_joint = Memo::new(move |_| !form.co_owners.get().is_empty());
    let can_share = owned && app.in_family();
    let tx_count = LocalResource::new(move || {
        app.rev.track();
        async move { api::transactions(&TxFilter { account_id: Some(id), limit: Some(1), ..Default::default() }).await.map(|p| p.total).unwrap_or(0) }
    });
    let summary = Signal::derive(move || form.summary(kind, &app));
    let save = move || {
        // "joint" here is only how a shared bank account is named: its fields are a bank account's
        let mut body = match form.body(if kind == "joint" { "bank" } else { kind }, has_others) {
            Ok(b) => b,
            Err(e) => return err.set(Some(e)),
        };
        if kind == "loan" {
            body.remove("balance");
        }
        if can_share && !is_joint.get_untracked() {
            body.insert("visibility".into(), form.visibility.get_untracked().into());
        }
        err.set(None);
        confirm.set(false);
        busy.set(true);
        leptos::task::spawn_local(async move {
            let r = api::patch::<Account>(&format!("/accounts/{id}"), &serde_json::Value::Object(body)).await;
            busy.set(false);
            match r {
                Ok(_) => {
                    saved.set(true);
                    app.reload();
                }
                Err(e) if e.status == 400 || e.status == 403 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let delete = {
        let nav = nav.clone();
        move |_| {
            if !confirm.get_untracked() {
                return confirm.set(true);
            }
            let nav = nav.clone();
            leptos::task::spawn_local(async move {
                match api::delete(&format!("/accounts/{id}")).await {
                    Ok(_) => {
                        app.reload();
                        nav("/accounts", Default::default());
                    }
                    Err(e) => app.fail(&e),
                }
            });
        }
    };
    let bal_label = match a.kind {
        AccountKind::Investment => "current value",
        AccountKind::Credit | AccountKind::Loan => "owed",
        AccountKind::Bank => "balance",
    };
    let owner_line = if a.owners.len() == 1 && owned { "yours".to_string() } else { owners_line(&a) };
    let (name, balance) = (a.name.clone(), app.money(a.balance));
    view! {
        <div style="display:flex;flex-direction:column;gap:16px">
            <div class="d-row" style="justify-content:space-between;gap:12px;align-items:flex-end">
                <div style="display:flex;flex-direction:column;gap:8px;align-items:flex-start">
                    <a class="d-btn d-btn--ghost d-btn--sm" href="/accounts"><Ico d=BACK small=true/>"all accounts"</a>
                    <div class="d-row" style="gap:12px">
                        <h1 class="heading-xl">{name}</h1>
                        <span class="d-badge">{kind_plural(kind)}</span>
                    </div>
                </div>
                <a class="d-btn d-btn--secondary" href=format!("/transactions?account={id}")>
                    "view transactions"<span style="color:var(--text-secondary);font-weight:400">{move || tx_count.get().map(|n| n.to_string()).unwrap_or_default()}</span>
                </a>
            </div>
            <div style="display:flex;flex-wrap:wrap;gap:16px;align-items:flex-start">
                <section class="d-card" aria-labelledby="ap-s" style="flex:2 1 320px">
                    <header class="d-card__head"><h2 class="d-card__title" id="ap-s">"summary"</h2><span class="d-card__meta">{owner_line}</span></header>
                    <div class="d-card__body" style="display:flex;flex-direction:column;gap:4px">
                        <span style="color:var(--text-secondary)">{bal_label}</span>
                        <span class="heading-3xl" style="color:var(--text-strong)">{balance}</span>
                    </div>
                    <div class="d-card__body d-card__body--flush" style="border-top:1px solid var(--border-default)"><SummaryList rows=summary/></div>
                </section>
                <form class="d-card" aria-labelledby="ap-d" on:submit=move |e| { e.prevent_default(); save() } on:input=move |_| saved.set(false) style="flex:3 1 420px">
                    <header class="d-card__head"><h2 class="d-card__title" id="ap-d">"details"</h2><span class="d-card__meta" role="status">{move || if saved.get() { "saved" } else { "" }}</span></header>
                    <fieldset disabled=!owned style="margin:0;padding:0;border:0;min-width:0">
                        <div class="d-card__body" style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(220px,100%),1fr));gap:16px;align-items:start">
                            <Fields form=form kind=kind.to_string() name_hint="" id="as"/>
                            {has_others.then(|| view! { <Owners form=form/> })}
                            {move || (can_share && !is_joint.get()).then(|| view! {
                                <div class="d-field">
                                    <label class="d-label" for="as-vis">"who sees it"</label>
                                    <span class="d-select">
                                        <select class="d-input" id="as-vis" on:change=move |e| form.visibility.set(event_target_value(&e))>
                                            <option value="private" selected=move || form.visibility.get() == "private">"only me"</option>
                                            <option value="shared" selected=move || form.visibility.get() == "shared">"the family"</option>
                                        </select>
                                    </span>
                                </div>
                            })}
                        </div>
                    </fieldset>
                    {move || err.get().map(|m| view! { <div class="d-card__body" style="padding-top:0"><span class="d-error" role="alert">{format!("error: {m}")}</span></div> })}
                    {move || confirm.get().then(|| view! { <div class="d-card__body" style="padding-top:0"><span class="d-error" role="alert">"this deletes the account and its transactions. it cannot be undone. press delete again to confirm."</span></div> })}
                    {if owned {
                        view! {
                            <footer class="d-card__foot" style="justify-content:space-between">
                                <button type="button" class="d-btn d-btn--danger" on:click=delete>{move || if confirm.get() { "confirm delete" } else { "delete account" }}</button>
                                <button type="submit" class="d-btn d-btn--primary" aria-busy=move || busy.get().then_some("true")>"save changes"</button>
                            </footer>
                        }.into_any()
                    } else {
                        view! { <div class="d-card__body" style="border-top:1px solid var(--border-default);color:var(--text-secondary)">"shared with you. only its owner can change it."</div> }.into_any()
                    }}
                </form>
            </div>
        </div>
    }
}

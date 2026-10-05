use dots_design::prelude::*;
use leptos::prelude::*;
use tracer_api::money::{format_minor, parse_minor};
use tracer_api::*;

use crate::api;
use crate::fmt;
use crate::state::AppState;

fn kind_label(k: AccountKind) -> &'static str {
    match k {
        AccountKind::Bank => "bank",
        AccountKind::Credit => "credit",
        AccountKind::Loan => "loans",
        AccountKind::Investment => "investments",
    }
}

#[component]
pub fn Accounts() -> impl IntoView {
    let app = expect_context::<AppState>();
    let tab = RwSignal::new("all".to_string());
    let editing = RwSignal::new(None::<Option<Account>>); // Some(None) = new
    let count = move |k: Option<AccountKind>| app.accounts.with(|a| a.iter().filter(|a| !a.archived && k.is_none_or(|k| a.kind == k)).count());
    let rows = move || {
        let t = tab.get();
        app.accounts.get().into_iter().filter(|a| !a.archived && (t == "all" || a.kind.as_str() == t)).collect::<Vec<_>>()
    };
    view! {
        <div style="display:flex;flex-direction:column;gap:16px">
            <div class="d-row" style="justify-content:space-between;gap:12px">
                <div style="display:flex;flex-direction:column;gap:4px">
                    <h1 class="heading-xl">"accounts"</h1>
                    <span style="color:var(--text-secondary)">{move || format!("{} accounts", count(None))}</span>
                </div>
                <Button variant=ButtonVariant::Primary on:click=move |_| editing.set(Some(None))>"add account"</Button>
            </div>
            <Tabs value=tab>
                <TabList label="account type">
                    <Tab value="all">{move || format!("all {}", count(None))}</Tab>
                    {[AccountKind::Bank, AccountKind::Credit, AccountKind::Loan, AccountKind::Investment].into_iter().map(|k| view! {
                        <Tab value=k.as_str()>{move || format!("{} {}", kind_label(k), count(Some(k)))}</Tab>
                    }).collect_view()}
                </TabList>
            </Tabs>
            {move || {
                let list = rows();
                if list.is_empty() {
                    return view! {
                        <EmptyState icon="wallet" title="no accounts here" hint="add the places your money lives: banks, cards, loans, investments.">
                            <Button variant=ButtonVariant::Primary on:click=move |_| editing.set(Some(None))>"add account"</Button>
                        </EmptyState>
                    }.into_any();
                }
                let family = app.has_family();
                view! {
                    <Table label="accounts" size=Size::Lg>
                        <thead><tr>
                            <Th>"name"</Th><Th>"kind"</Th>
                            {family.then(|| view! { <Th>"owners"</Th> })}
                            <Th>"details"</Th><Th num=true>"balance"</Th><Th><span class="d-sr">"edit"</span></Th>
                        </tr></thead>
                        <tbody>
                            {list.into_iter().map(|a| {
                                let edit = a.clone();
                                let details = detail_text(&a, &app);
                                let balance = if a.kind.is_liability() { format!("owe {}", app.money(a.balance)) } else { app.money(a.balance) };
                                let owners = a.owners.iter().map(|o| o.name.split(' ').next().unwrap_or("").to_string()).collect::<Vec<_>>().join(", ");
                                let vis = match (a.joint, a.visibility) { (true, _) => "joint", (_, Visibility::Shared) => "shared", _ => "private" };
                                let (name, edit_label, kind_name) = (a.name.clone(), format!("edit {}", a.name), a.kind.as_str());
                                view! {
                                    <Tr>
                                        <Td>{name}</Td>
                                        <Td><Badge>{kind_name}</Badge>" "<Badge>{vis}</Badge></Td>
                                        {family.then(|| view! { <Td>{owners}</Td> })}
                                        <Td><span style="color:var(--text-secondary)">{details}</span></Td>
                                        <Td num=true>{balance}</Td>
                                        <Td><Button variant=ButtonVariant::Ghost size=Size::Sm icon=true label=edit_label on:click=move |_| editing.set(Some(Some(edit.clone())))><Icon name="ellipsis"/></Button></Td>
                                    </Tr>
                                }
                            }).collect_view()}
                        </tbody>
                    </Table>
                }.into_any()
            }}
        </div>
        <AccountSheet editing=editing/>
    }
}

fn detail_text(a: &Account, app: &AppState) -> String {
    let d = &a.details;
    match a.kind {
        AccountKind::Credit => {
            let mut parts = vec![];
            if let Some(l) = d.limit {
                parts.push(format!("limit {}", app.money(l)));
            }
            if let Some(due) = d.due_day {
                parts.push(format!("due on the {due}"));
            }
            parts.join(" · ")
        }
        AccountKind::Loan => a.loan.as_ref().map(|l| format!("emi {} · {} months left · ends {}", app.money(l.emi), l.left, l.end)).unwrap_or_default(),
        AccountKind::Investment => {
            let mut parts = vec![];
            if !d.invest_kind.is_empty() {
                parts.push(d.invest_kind.clone());
            }
            if let Some(i) = d.invested {
                parts.push(format!("put in {}", app.money(i)));
            }
            parts.join(" · ")
        }
        AccountKind::Bank => d.institution.clone(),
    }
}

#[component]
fn AccountSheet(editing: RwSignal<Option<Option<Account>>>) -> impl IntoView {
    let open = RwSignal::new(false);
    Effect::new(move |_| open.set(editing.get().is_some()));
    Effect::new(move |_| {
        if !open.get() && editing.get_untracked().is_some() {
            editing.set(None);
        }
    });
    view! {
        <Sheet open=open title="account">
            {move || editing.get().map(|a| view! { <AccountForm account=a editing=editing/> })}
        </Sheet>
    }
}

fn opt(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

#[component]
fn AccountForm(account: Option<Account>, editing: RwSignal<Option<Option<Account>>>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let id = account.as_ref().map(|a| a.id);
    let d = account.as_ref().map(|a| a.details.clone()).unwrap_or_default();
    let money_in = |v: Option<i64>| v.map(fmt::plain).unwrap_or_default();
    let kind = RwSignal::new(account.as_ref().map(|a| a.kind.as_str().to_string()).unwrap_or_else(|| "bank".into()));
    let name = RwSignal::new(account.as_ref().map(|a| a.name.clone()).unwrap_or_default());
    let balance = RwSignal::new(account.as_ref().filter(|a| a.kind != AccountKind::Loan).map(|a| fmt::plain(a.balance)).unwrap_or_default());
    let institution = RwSignal::new(d.institution.clone());
    let last4 = RwSignal::new(d.last4.clone());
    let limit = RwSignal::new(money_in(d.limit));
    let stmt = RwSignal::new(d.statement_day.map(|v| v.to_string()).unwrap_or_default());
    let due = RwSignal::new(d.due_day.map(|v| v.to_string()).unwrap_or_default());
    let total = RwSignal::new(money_in(d.loan_total));
    let rate = RwSignal::new(d.rate.map(|v| v.to_string()).unwrap_or_default());
    let tenure = RwSignal::new(d.tenure.map(|v| v.to_string()).unwrap_or_default());
    let start = RwSignal::new(d.start.clone().unwrap_or_default());
    let emi = RwSignal::new(money_in(d.emi));
    let emi_day = RwSignal::new(d.emi_day.map(|v| v.to_string()).unwrap_or_default());
    let invest_kind = RwSignal::new(d.invest_kind.clone());
    let invested = RwSignal::new(money_in(d.invested));
    let sip = RwSignal::new(money_in(d.sip));
    let visibility = RwSignal::new(account.as_ref().map(|a| a.visibility.as_str().to_string()).unwrap_or_else(|| "private".into()));
    let co_owners = RwSignal::new(account.as_ref().map(|a| a.owners.iter().map(|o| o.id).filter(|o| *o != app.my_id()).collect::<Vec<_>>()).unwrap_or_default());
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let confirm = RwSignal::new(false);
    let owned = account.as_ref().is_none_or(|a| a.owners.iter().any(|o| o.id == app.my_id()));

    let is = move |k: &str| kind.get() == k;
    let preview = move || {
        if !is("loan") {
            return None;
        }
        let t = parse_minor(&total.get()).ok()?;
        let (y, m) = { let n = js_sys::Date::new_0(); (n.get_full_year() as i32, n.get_month() + 1) };
        tracer_api::loan::compute(t, rate.get().parse().unwrap_or(0.0), tenure.get().parse().ok()?, &start.get(), parse_minor(&emi.get()).ok().filter(|e| *e > 0), (y, m))
    };

    let save = move |_| {
        if name.get_untracked().trim().is_empty() {
            return err.set(Some("name the account".into()));
        }
        let k = kind.get_untracked();
        let amount = |s: RwSignal<String>| -> Result<Option<String>, String> {
            match opt(&s.get_untracked()) {
                None => Ok(None),
                Some(v) => parse_minor(&v).map(|m| Some(format_minor(m))).map_err(|e| e),
            }
        };
        let build = || -> Result<serde_json::Value, String> {
            let mut o = serde_json::Map::new();
            o.insert("name".into(), name.get_untracked().into());
            o.insert("institution".into(), institution.get_untracked().into());
            if let Some(b) = amount(balance)? {
                if k != "loan" {
                    o.insert("balance".into(), b.into());
                }
            }
            match k.as_str() {
                "credit" => {
                    o.insert("last4".into(), last4.get_untracked().into());
                    if let Some(v) = amount(limit)? { o.insert("limit".into(), v.into()); }
                    if let Ok(v) = stmt.get_untracked().trim().parse::<u32>() { o.insert("statement_day".into(), v.into()); }
                    if let Ok(v) = due.get_untracked().trim().parse::<u32>() { o.insert("due_day".into(), v.into()); }
                }
                "loan" => {
                    if let Some(v) = amount(total)? { o.insert("loan_total".into(), v.into()); }
                    if let Ok(v) = rate.get_untracked().trim().parse::<f64>() { o.insert("rate".into(), v.into()); }
                    if let Ok(v) = tenure.get_untracked().trim().parse::<u32>() { o.insert("tenure".into(), v.into()); }
                    if let Some(v) = opt(&start.get_untracked()) { o.insert("start".into(), v.chars().take(7).collect::<String>().into()); }
                    if let Some(v) = amount(emi)? { o.insert("emi".into(), v.into()); }
                    if let Ok(v) = emi_day.get_untracked().trim().parse::<u32>() { o.insert("emi_day".into(), v.into()); }
                }
                "investment" => {
                    o.insert("invest_kind".into(), invest_kind.get_untracked().into());
                    if let Some(v) = amount(invested)? { o.insert("invested".into(), v.into()); }
                    if let Some(v) = amount(sip)? { o.insert("sip".into(), v.into()); }
                }
                _ => {}
            }
            if app.has_family() {
                o.insert("visibility".into(), visibility.get_untracked().into());
                o.insert("owner_ids".into(), co_owners.get_untracked().into());
            }
            Ok(serde_json::Value::Object(o))
        };
        let mut body = match build() {
            Ok(b) => b,
            Err(e) => return err.set(Some(e)),
        };
        busy.set(true);
        err.set(None);
        leptos::task::spawn_local(async move {
            let r = match id {
                Some(id) => api::patch::<Account>(&format!("/accounts/{id}"), &body).await,
                None => {
                    body["kind"] = k.clone().into();
                    api::post::<Account>("/accounts", &body).await
                }
            };
            busy.set(false);
            match r {
                Ok(_) => {
                    app.ok(if id.is_some() { "saved" } else { "account added" });
                    editing.set(None);
                    app.reload();
                }
                Err(e) if e.status == 400 || e.status == 403 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let archive = move |_| {
        let Some(id) = id else { return };
        leptos::task::spawn_local(async move {
            match api::patch::<Account>(&format!("/accounts/{id}"), &serde_json::json!({"archived": true})).await {
                Ok(_) => {
                    app.ok("archived");
                    confirm.set(false);
                    editing.set(None);
                    app.reload();
                }
                Err(e) => app.fail(&e),
            }
        });
    };
    let kind_options = Signal::stored(vec![SelectOption::new("bank", "bank account"), SelectOption::new("credit", "credit card"), SelectOption::new("loan", "loan"), SelectOption::new("investment", "investment")]);
    let balance_label = move || match kind.get().as_str() {
        "credit" => "owed now",
        "investment" => "current value",
        _ => "balance",
    };
    view! {
        <form class="d-stack" style="gap:var(--space-16)" on:submit=move |e| { e.prevent_default(); save(()) }>
            {id.is_none().then(|| view! { <Select label="what is it" value=kind options=kind_options/> })}
            <TextField label="name" value=name placeholder="salary account"/>
            <TextField label="institution" value=institution optional=true placeholder="hdfc"/>
            {move || (!is("loan")).then(|| view! {
                <TextField label=balance_label() value=balance placeholder="0" hint=Signal::derive(move || id.is_some().then(|| "changing it keeps your history; it moves the starting figure.".to_string()))/>
            })}
            {move || is("credit").then(|| view! {
                <div class="d-row" style="gap:var(--space-12);align-items:flex-start">
                    <div style="flex:1"><TextField label="limit" value=limit/></div>
                    <div style="flex:1"><TextField label="last 4 digits" value=last4 optional=true/></div>
                </div>
                <div class="d-row" style="gap:var(--space-12);align-items:flex-start">
                    <div style="flex:1"><TextField label="statement day" value=stmt optional=true hint="day of month"/></div>
                    <div style="flex:1"><TextField label="due day" value=due optional=true hint="day of month"/></div>
                </div>
            })}
            {move || is("loan").then(|| view! {
                <div class="d-row" style="gap:var(--space-12);align-items:flex-start">
                    <div style="flex:1"><TextField label="amount borrowed" value=total/></div>
                    <div style="flex:1"><TextField label="interest rate, % a year" value=rate/></div>
                </div>
                <div class="d-row" style="gap:var(--space-12);align-items:flex-start">
                    <div style="flex:1"><TextField label="tenure, months" value=tenure/></div>
                    <div style="flex:1"><TextField label="first emi month" value=start input_type="month"/></div>
                </div>
                <div class="d-row" style="gap:var(--space-12);align-items:flex-start">
                    <div style="flex:1"><TextField label="emi" value=emi optional=true hint="leave empty to work it out"/></div>
                    <div style="flex:1"><TextField label="emi day" value=emi_day optional=true hint="day of month"/></div>
                </div>
                {move || preview().map(|p| view! {
                    <div class="d-card"><div class="d-card__body d-stack" style="gap:var(--space-4)">
                        <b>"what this sets up"</b>
                        <span>{format!("emi {}", app.money(p.emi))}</span>
                        <span>{format!("{} of {} months paid, {} left", p.paid, p.tenure, p.left)}</span>
                        <span>{format!("owed now {}, last emi {}", app.money(p.balance), p.end)}</span>
                        <span class="d-hint">{format!("interest over the loan {}", app.money(p.interest))}</span>
                    </div></div>
                })}
            })}
            {move || is("investment").then(|| view! {
                <TextField label="what kind" value=invest_kind optional=true placeholder="mutual fund, ppf, fixed deposit"/>
                <div class="d-row" style="gap:var(--space-12);align-items:flex-start">
                    <div style="flex:1"><TextField label="put in so far" value=invested optional=true/></div>
                    <div style="flex:1"><TextField label="monthly sip" value=sip optional=true/></div>
                </div>
            })}
            {move || (app.has_family() && owned).then(|| view! {
                <div class="d-field">
                    <span class="d-label">"who sees it"</span>
                    <RadioGroup label="visibility" value=visibility>
                        <Radio value="private">"only me"</Radio>
                        <Radio value="shared">"my family can see it"</Radio>
                    </RadioGroup>
                </div>
                <div class="d-field">
                    <span class="d-label">"joint with"</span>
                    {app.members().into_iter().filter(|m| m.id != app.my_id()).map(|m| {
                        let mid = m.id;
                        let on = RwSignal::new(co_owners.get_untracked().contains(&mid));
                        Effect::new(move |_| co_owners.update(|v| { v.retain(|x| *x != mid); if on.get() { v.push(mid); } }));
                        view! { <Checkbox checked=on>{m.name.clone()}</Checkbox> }
                    }).collect_view()}
                    <span class="d-hint">"both of you can add transactions to a joint account"</span>
                </div>
            })}
            {move || err.get().map(|m| view! { <span class="d-error" role="alert"><Icon name="octagon-alert" small=true/>{format!("error: {m}")}</span> })}
            <div class="d-row" style="justify-content:space-between">
                <div>{(id.is_some() && owned).then(|| view! { <Button variant=ButtonVariant::Danger on:click=move |_| confirm.set(true)>"archive account"</Button> })}</div>
                <div class="d-row" style="gap:var(--space-8)">
                    <Button on:click=move |_| editing.set(None)>"cancel"</Button>
                    {owned.then(|| view! { <Button variant=ButtonVariant::Primary submit=true busy=busy>{if id.is_some() { "save" } else { "add account" }}</Button> })}
                </div>
            </div>
        </form>
        <Dialog open=confirm title="archive account" footer=move || view! {
            <Button on:click=move |_| confirm.set(false)>"cancel"</Button>
            <Button variant=ButtonVariant::Danger on:click=archive>"archive account"</Button>
        }>
            <p>"it leaves your lists and totals. its transactions are kept."</p>
        </Dialog>
    }
}

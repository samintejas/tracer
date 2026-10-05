use dots_design::prelude::*;
use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use tracer_api::money::{format_minor, parse_minor};
use tracer_api::*;
use wasm_bindgen::JsCast;

use crate::api;
use crate::fmt;
use crate::icons::*;
use crate::state::AppState;

fn join(v: &[String]) -> Option<String> {
    (!v.is_empty()).then(|| v.join(","))
}

fn parse_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in s.split(',') {
        let t = t.trim().to_lowercase();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// The add bar's fields, shared so a row's "repeat" can fill them.
#[derive(Clone, Copy)]
struct Quick {
    kind: RwSignal<String>,
    date: RwSignal<String>,
    desc: RwSignal<String>,
    tags: RwSignal<Vec<String>>,
    tags_touched: RwSignal<bool>,
    acct: RwSignal<String>,
    to: RwSignal<String>,
    amt: RwSignal<String>,
    err: RwSignal<Option<String>>,
    desc_ref: NodeRef<leptos::html::Input>,
}

impl Quick {
    fn new() -> Self {
        Quick {
            kind: RwSignal::new("debit".into()),
            date: RwSignal::new(fmt::today()),
            desc: RwSignal::new(String::new()),
            tags: RwSignal::new(Vec::new()),
            tags_touched: RwSignal::new(false),
            acct: RwSignal::new(String::new()),
            to: RwSignal::new(String::new()),
            amt: RwSignal::new(String::new()),
            err: RwSignal::new(None),
            desc_ref: NodeRef::new(),
        }
    }

    fn focus(&self) {
        if let Some(i) = self.desc_ref.get_untracked() {
            let _ = i.focus();
        }
    }
}

#[component]
pub fn Transactions() -> impl IntoView {
    let app = expect_context::<AppState>();
    let quick = Quick::new();

    // filters
    let q = RwSignal::new(String::new());
    let kinds = RwSignal::new(Vec::<String>::new());
    let accts = RwSignal::new(Vec::<String>::new());
    let people = RwSignal::new(Vec::<String>::new());
    let tag_sel = RwSignal::new(Vec::<String>::new());
    let from = RwSignal::new(String::new());
    let to = RwSignal::new(String::new());
    let sort_key = RwSignal::new("date");
    let sort_dir = RwSignal::new(SortDirection::Descending);
    let page = RwSignal::new(1usize);
    let page_size = RwSignal::new(10usize);

    // "view transactions" on an account arrives as ?account=3
    let query = use_query_map();
    Effect::new(move |_| {
        if let Some(a) = query.with(|q| q.get("account")) {
            accts.set(vec![a]);
        }
    });

    let base = Memo::new(move |_| TxFilter {
        q: Some(q.get()).filter(|s| !s.trim().is_empty()),
        kinds: join(&kinds.get()),
        accounts: join(&accts.get()),
        tags: join(&tag_sel.get()),
        member_id: people.get().first().and_then(|p| p.parse().ok()),
        from: Some(from.get()).filter(|s| !s.is_empty()),
        to: Some(to.get()).filter(|s| !s.is_empty()),
        collapse_transfers: Some(true),
        sort: Some(sort_key.get().to_string()),
        dir: Some(if sort_dir.get() == SortDirection::Ascending { "asc" } else { "desc" }.into()),
        ..Default::default()
    });
    // a new filter or sort starts again at the first page
    Effect::new(move |prev: Option<()>| {
        base.track();
        if prev.is_some() {
            page.set(1);
        }
    });
    let data = LocalResource::new(move || {
        let mut f = base.get();
        let (p, s) = (page.get(), page_size.get());
        f.limit = Some(s as u32);
        f.offset = Some(((p - 1) * s) as u32);
        app.rev.track();
        async move { api::transactions(&f).await }
    });
    // keep the last good page on screen while the next one loads
    let shown = RwSignal::new(None::<Result<TxPage, api::ApiError>>);
    Effect::new(move |_| {
        if let Some(r) = data.get() {
            shown.set(Some(r));
        }
    });
    let total = Memo::new(move |_| shown.with(|s| s.as_ref().and_then(|r| r.as_ref().ok()).map(|p| p.total as usize).unwrap_or(0)));
    let pages = Memo::new(move |_| total.get().div_ceil(page_size.get().max(1)).max(1));
    Effect::new(move |_| {
        if page.get() > pages.get() {
            page.set(pages.get());
        }
    });
    let any_filter = Memo::new(move |_| !q.get().trim().is_empty() || !kinds.get().is_empty() || !accts.get().is_empty() || !people.get().is_empty() || !tag_sel.get().is_empty() || !from.get().is_empty() || !to.get().is_empty());
    let clear_all = move || {
        q.set(String::new());
        for s in [kinds, accts, people, tag_sel] {
            s.set(Vec::new());
        }
        from.set(String::new());
        to.set(String::new());
    };
    let summary = move || match shown.get() {
        Some(Ok(p)) => format!("{} {} · in {} · out {}", p.total, if p.total == 1 { "entry" } else { "entries" }, app.money(p.total_in), app.money(p.total_out)),
        _ => String::new(),
    };

    let th = move |key: &'static str| {
        let dir = Signal::derive(move || if sort_key.get() == key { sort_dir.get() } else { SortDirection::None });
        let on = Callback::new(move |()| {
            if sort_key.get_untracked() == key {
                sort_dir.update(|d| *d = d.next());
            } else {
                sort_key.set(key);
                sort_dir.set(if key == "date" || key == "amount" { SortDirection::Descending } else { SortDirection::Ascending });
            }
        });
        (dir, on)
    };

    let kind_options = Signal::stored(vec![FilterOption::new("debit", "debit").meta("money out"), FilterOption::new("credit", "credit").meta("money in"), FilterOption::new("transfer", "transfer").meta("between accounts")]);
    let acct_options = Signal::derive(move || {
        app.accounts.get().into_iter().filter(|a| !a.archived).map(|a| FilterOption::new(a.id.to_string(), a.name.clone()).meta(format!("{}{}", if a.kind.is_liability() { "−" } else { "" }, app.money(a.balance)))).collect::<Vec<_>>()
    });
    let tag_options = Signal::derive(move || app.tags.get().into_iter().map(|t| FilterOption::new(t.clone(), t.clone()).cat(tag_cat(&t))).collect::<Vec<_>>());
    let people_options = Signal::derive(move || app.members().into_iter().map(|m| FilterOption::new(m.id.to_string(), m.name.clone())).collect::<Vec<_>>());
    let range_label = Memo::new(move |_| range_text(&from.get(), &to.get()));

    let no_accounts = Memo::new(move |_| app.me.get().is_some() && app.accounts.with(|a| a.iter().all(|a| a.archived)));
    let family = Memo::new(move |_| app.has_family());

    view! {
        <div style="display:flex;flex-wrap:wrap;gap:16px;align-items:flex-start">
            <div class="tx-wrap" style="flex:999 1 560px">
                <div style="display:flex;flex-direction:column;gap:4px">
                    <h1 class="heading-xl">"transactions"</h1>
                    <span style="color:var(--text-secondary)">{summary}" "</span>
                </div>
                {move || if no_accounts.get() {
                    view! {
                        <EmptyState title="no accounts yet" hint="a transaction needs an account to sit in. add your first account, then come back here.">
                            <ButtonLink href="/accounts/new">"add your first account"</ButtonLink>
                        </EmptyState>
                    }.into_any()
                } else {
                    view! {
                        <QuickAdd quick=quick/>
                        <FilterBar label="filters">
                            <FilterSearch label="search transactions" value=q placeholder="search description or tag"/>
                            <Filter label="type" options=kind_options selected=kinds/>
                            <Filter label="account" options=acct_options selected=accts/>
                            <Filter label="tags" options=tag_options selected=tag_sel/>
                            {move || family.get().then(|| view! { <Filter label="person" options=people_options selected=people single=true empty="everyone"/> })}
                            <DateFilter from=from to=to label=range_label/>
                            {move || any_filter.get().then(|| view! { <Button variant=ButtonVariant::Ghost on:click=move |_| clear_all()>"clear all"</Button> })}
                            {move || any_filter.get().then(|| view! {
                                <FilterActive>
                                    {move || kinds.get().into_iter().map(|k| { let k2 = k.clone(); view! { <Tag text=format!("type: {k}") on_remove=move |()| kinds.update(|v| v.retain(|x| *x != k2))/> } }).collect_view()}
                                    {move || accts.get().into_iter().map(|a| { let a2 = a.clone(); view! { <Tag text=format!("account: {}", app.account_name(a.parse().unwrap_or(0))) on_remove=move |()| accts.update(|v| v.retain(|x| *x != a2))/> } }).collect_view()}
                                    {move || tag_sel.get().into_iter().map(|t| { let t2 = t.clone(); view! { <Tag text=t on_remove=move |()| tag_sel.update(|v| v.retain(|x| *x != t2))/> } }).collect_view()}
                                    {move || people.get().into_iter().map(|p| { let name = people_options.get().into_iter().find(|o| o.value == p).map(|o| o.label).unwrap_or_default(); view! { <Tag text=format!("person: {}", name.split(' ').next().unwrap_or("")) on_remove=move |()| people.set(Vec::new())/> } }).collect_view()}
                                    {move || (!from.get().is_empty() || !to.get().is_empty()).then(|| view! { <Tag text=format!("date: {}", range_label.get()) on_remove=move |()| { from.set(String::new()); to.set(String::new()); }/> })}
                                    {move || (!q.get().trim().is_empty()).then(|| view! { <Tag text=format!("search: {}", q.get().trim()) on_remove=move |()| q.set(String::new())/> })}
                                </FilterActive>
                            })}
                        </FilterBar>
                        {move || match shown.get() {
                            None => view! { <EmptyLoading title="loading transactions"/> }.into_any(),
                            Some(Err(e)) => view! { <EmptyError title="could not load transactions" hint=e.message/> }.into_any(),
                            Some(Ok(p)) if p.items.is_empty() && !any_filter.get() => view! {
                                <EmptyState title="no transactions yet" hint="type your first one in the bar above and press enter. start the amount with + for money in.">
                                    <Button on:click=move |_| quick.focus()>"go to the bar"</Button>
                                </EmptyState>
                            }.into_any(),
                            Some(Ok(p)) if p.items.is_empty() => view! {
                                <EmptyState title="no transactions match" hint="change the search or clear the filters.">
                                    <Button on:click=move |_| clear_all()>"clear filters"</Button>
                                </EmptyState>
                            }.into_any(),
                            Some(Ok(p)) => {
                                let cols = [th("date"), th("description"), th("tag"), th("account"), th("person"), th("amount")];
                                let fam = family.get();
                                view! {
                                    <div class="d-table-wrap" tabindex="0" role="region" aria-label="transactions. select a field to edit it in place" style="flex:0 1 auto;min-height:0;overscroll-behavior:contain">
                                        <table class="d-table d-table--lg">
                                            <thead>
                                                <tr>
                                                    <Th sort=cols[0].0 on_sort=cols[0].1>"date"</Th>
                                                    <Th sort=cols[1].0 on_sort=cols[1].1>"description"</Th>
                                                    <Th sort=cols[2].0 on_sort=cols[2].1>"tags"</Th>
                                                    <Th sort=cols[3].0 on_sort=cols[3].1>"account"</Th>
                                                    {fam.then(|| view! { <Th sort=cols[4].0 on_sort=cols[4].1>"person"</Th> })}
                                                    <Th num=true sort=cols[5].0 on_sort=cols[5].1>"amount"</Th>
                                                    <th scope="col"><span class="d-sr">"open"</span></th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                {p.items.into_iter().map(|t| view! { <Row t=t family=fam quick=quick/> }).collect_view()}
                                            </tbody>
                                        </table>
                                    </div>
                                }.into_any()
                            }
                        }}
                        <div class="d-row" style="flex:none;margin-top:auto;justify-content:space-between;gap:12px;padding:12px 0;background:var(--bg-base);border-top:1px solid var(--border-default)">
                            <span style="color:var(--text-secondary)">{move || {
                                let (t, s, p) = (total.get(), page_size.get(), page.get());
                                if t == 0 { String::new() } else { format!("{}–{} of {}", (p - 1) * s + 1, (p * s).min(t), t) }
                            }}</span>
                            <nav class="d-row" aria-label="pages" style="gap:4px;flex-wrap:nowrap">
                                <span class="d-select" style="width:64px;margin-right:8px" title="rows per page">
                                    <select class="d-input" aria-label="rows per page" on:change=move |e| { if let Ok(n) = event_target_value(&e).parse() { page_size.set(n); page.set(1); } }>
                                        {[10usize, 25, 50].into_iter().map(|n| view! { <option value=n.to_string() selected=move || page_size.get() == n>{n.to_string()}</option> }).collect_view()}
                                    </select>
                                </span>
                                <button type="button" class="d-btn d-btn--ghost d-btn--icon" aria-label="previous page" title="previous" disabled={move || page.get() <= 1} on:click=move |_| page.update(|p| *p -= 1)><Ico d=PREV/></button>
                                <span style="min-width:48px;text-align:center;color:var(--text-strong)">{move || format!("{} / {}", page.get(), pages.get())}</span>
                                <button type="button" class="d-btn d-btn--ghost d-btn--icon" aria-label="next page" title="next" disabled={move || page.get() >= pages.get()} on:click=move |_| page.update(|p| *p += 1)><Ico d=NEXT/></button>
                            </nav>
                        </div>
                    }.into_any()
                }}
            </div>
        </div>
    }
}

fn range_text(from: &str, to: &str) -> String {
    let today = fmt::today();
    let month_start = format!("{}-01", &today[..7]);
    match (from, to) {
        ("", "") => "all time".into(),
        (f, t) if f == month_start && t == today => "this month".into(),
        (f, t) if f == fmt::days_ago(30) && t == today => "last 30 days".into(),
        (f, "") => format!("from {}", fmt::day(f)),
        ("", t) => format!("until {}", fmt::day(t)),
        (f, t) => format!("{} to {}", fmt::day(f), fmt::day(t)),
    }
}

/// The date filter: three quick ranges, or any from and to.
#[component]
fn DateFilter(from: RwSignal<String>, to: RwSignal<String>, label: Memo<String>) -> impl IntoView {
    let open = RwSignal::new(false);
    let root = NodeRef::<leptos::html::Div>::new();
    let click = window_event_listener(leptos::ev::pointerdown, move |e| {
        let inside = match (root.get_untracked(), e.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok())) {
            (Some(r), Some(t)) => r.contains(Some(&t)),
            _ => false,
        };
        if !inside && open.get_untracked() {
            open.set(false);
        }
    });
    on_cleanup(move || click.remove());
    let today = fmt::today();
    let ranges = [("this month", format!("{}-01", &today[..7]), today.clone()), ("last 30 days", fmt::days_ago(30), today.clone()), ("all time", String::new(), String::new())];
    let active = move || !from.get().is_empty() || !to.get().is_empty();
    view! {
        <div style="position:relative" node_ref=root on:keydown=move |e| if e.key() == "Escape" { open.set(false) }>
            <button type="button" class="d-btn d-btn--secondary d-filter" aria-haspopup="dialog" aria-expanded=move || open.get().to_string()
                data-active=move || active().then_some("") on:click=move |_| open.update(|o| *o = !*o)>
                "date "<span class="d-filter__value">{move || label.get()}</span>
            </button>
            {move || open.get().then(|| {
                let ranges = ranges.clone();
                view! {
                    <div class="d-menu" role="dialog" aria-label="date range" style="position:absolute;top:calc(100% + 4px);left:0;z-index:200;width:280px;max-width:none">
                        <div class="d-menu__label" role="presentation">"quick ranges"</div>
                        {ranges.into_iter().map(|(name, f, t)| {
                            let (f2, t2) = (f.clone(), t.clone());
                            view! {
                                <button type="button" class="d-menu__item" role="menuitemradio" aria-checked=move || (from.get() == f && to.get() == t).to_string()
                                    on:click=move |_| { from.set(f2.clone()); to.set(t2.clone()); }>
                                    <span class="d-menu__mark"></span>{name}
                                </button>
                            }
                        }).collect_view()}
                        <div class="d-menu__sep" role="separator"></div>
                        <div class="d-menu__label" role="presentation">"custom"</div>
                        <div style="display:flex;flex-direction:column;gap:8px;padding:0 12px 8px">
                            <div class="d-field">
                                <label class="d-label" for="tx-from">"from"</label>
                                <input class="d-input" id="tx-from" type="date" max=fmt::today() prop:value=move || from.get() on:change=move |e| from.set(event_target_value(&e))/>
                            </div>
                            <div class="d-field">
                                <label class="d-label" for="tx-to">"to"</label>
                                <input class="d-input" id="tx-to" type="date" max=fmt::today() prop:value=move || to.get() on:change=move |e| to.set(event_target_value(&e))/>
                            </div>
                        </div>
                        <div class="d-row" style="justify-content:flex-end;padding:0 12px 8px">
                            <button type="button" class="d-btn d-btn--secondary d-btn--sm" on:click=move |_| open.set(false)>"done"</button>
                        </div>
                    </div>
                }
            })}
        </div>
    }
}

#[component]
fn Row(t: Transaction, family: bool, quick: Quick) -> impl IntoView {
    let app = expect_context::<AppState>();
    let (id, amount) = (t.id, t.amount);
    let is_transfer = t.kind == TxKind::Transfer;
    let save = move |body: serde_json::Value| {
        leptos::task::spawn_local(async move {
            match api::patch::<Transaction>(&format!("/transactions/{id}"), &body).await {
                Ok(_) => app.reload(),
                Err(e) if e.status == 400 || e.status == 403 => app.error(e.message),
                Err(e) => app.fail(&e),
            }
        });
    };
    let desc = t.description.clone();
    let desc_shown = if desc.is_empty() { "—".to_string() } else { desc.clone() };
    let tags_raw = t.tags.join(", ");
    let tags = StoredValue::new(t.tags.clone());
    let account = match t.counterpart_id {
        Some(c) if is_transfer => format!("{} → {}", app.account_name(t.account_id), app.account_name(c)),
        _ => app.account_name(t.account_id),
    };
    let amount_shown = format!("{}{}", if is_transfer { "" } else if amount > 0 { "+" } else { "−" }, app.money(amount));
    let (first, initials) = (t.created_by.name.split(' ').next().unwrap_or("").to_string(), t.created_by.initials.clone());
    let editing_acct = RwSignal::new(false);
    let acct_id = t.account_id.to_string();
    let acct_id2 = acct_id.clone();
    let open = t.clone();
    let dup = t.clone();
    let (dup_label, open_label) = (format!("duplicate {} into the add bar", t.description), format!("open {} in the side panel", t.description));
    let date_shown = fmt::day(&t.date);
    let today = fmt::today();
    view! {
        <tr aria-selected=move || app.panel.with(|p| p.as_ref().is_some_and(|p| p.id == id)).to_string()>
            <EditCell label="date" kind=EditKind::Date value=t.date.clone() on_save=move |v: String| {
                if v.is_empty() || v > today { app.error("error: the date cannot be in the future"); } else { save(serde_json::json!({"date": v})) }
            }>{date_shown.clone()}</EditCell>
            <EditCell label="description" value=desc on_save=move |v: String| if !v.trim().is_empty() { save(serde_json::json!({"description": v.trim().to_lowercase()})) }>
                <span style="color:var(--text-strong)">{desc_shown.clone()}</span>
            </EditCell>
            <EditCell label="tags, separated by commas" value=tags_raw on_save=move |v: String| { let list = parse_tags(&v); if !list.is_empty() { save(serde_json::json!({"tags": list})) } }>
                <span class="d-row" style="gap:4px;flex-wrap:nowrap">
                    {tags.get_value().into_iter().map(|g| { let c = tag_cat(&g); view! { <span class="d-badge" data-cat=c.to_string()>{g}</span> } }).collect_view()}
                </span>
            </EditCell>
            <td>
                {move || if editing_acct.get() && !is_transfer {
                    let current = acct_id.clone();
                    view! {
                        <span class="d-select">
                            <select class="d-input cell-in" aria-label="account" autofocus=true
                                on:change=move |e| { editing_acct.set(false); if let Ok(a) = event_target_value(&e).parse::<i64>() { save(serde_json::json!({"account_id": a})) } }
                                on:blur=move |_| editing_acct.set(false)
                                on:keydown=move |e| if e.key() == "Escape" { editing_acct.set(false) }>
                                {app.mine().into_iter().map(|a| { let v = a.id.to_string(); let sel = v == current; view! { <option value=v selected=sel>{a.name.clone()}</option> } }).collect_view()}
                            </select>
                        </span>
                    }.into_any()
                } else {
                    let _ = &acct_id2;
                    view! { <button type="button" class="cell-btn" title=if is_transfer { "a transfer keeps its accounts" } else { "edit account" } on:click=move |_| editing_acct.set(true)>{account.clone()}</button> }.into_any()
                }}
            </td>
            {family.then(|| view! { <td><span class="d-row" style="gap:8px;flex-wrap:nowrap"><span class="d-avatar d-avatar--sm" aria-hidden="true">{initials}</span>{first}</span></td> })}
            <EditCell label="amount" kind=EditKind::Number num=true value=fmt::plain(amount) on_save=move |v: String| match parse_minor(&v) {
                Ok(a) if a > 0 => save(serde_json::json!({"amount": format_minor(a)})),
                _ => app.error("error: enter an amount above zero"),
            }>
                <span style="color:var(--text-strong)">{amount_shown.clone()}</span>
            </EditCell>
            <td class="d-num" style="white-space:nowrap">
                <button type="button" class="d-btn d-btn--ghost d-btn--icon d-btn--sm" aria-label=dup_label title="repeat: fill the add bar with this transaction, dated today"
                    on:click=move |_| {
                        quick.kind.set(if dup.kind == TxKind::Transfer { "transfer" } else { "debit" }.into());
                        quick.desc.set(dup.description.clone());
                        quick.tags.set(dup.tags.clone());
                        quick.tags_touched.set(true);
                        quick.acct.set(dup.account_id.to_string());
                        if let Some(c) = dup.counterpart_id { quick.to.set(c.to_string()); }
                        quick.amt.set(format!("{}{}", if dup.kind == TxKind::Credit { "+" } else { "" }, fmt::plain(dup.amount)));
                        quick.date.set(fmt::today());
                        quick.err.set(None);
                        quick.focus();
                    }><Ico d=REPEAT/></button>
                <button type="button" class="d-btn d-btn--ghost d-btn--icon d-btn--sm" aria-label=open_label title="open in the side panel" on:click=move |_| app.panel.set(Some(open.clone()))><Ico d=PANEL_RIGHT/></button>
            </td>
        </tr>
    }
}

/// What a transfer is called when nobody typed a description. The server uses the same words.
fn auto_description(to: &Account) -> String {
    match to.kind {
        AccountKind::Credit => format!("card bill, {}", to.name),
        AccountKind::Loan => format!("emi, {}", to.name),
        AccountKind::Investment => format!("invested in {}", to.name),
        AccountKind::Bank => format!("to {}", to.name),
    }
}

#[component]
fn QuickAdd(quick: Quick) -> impl IntoView {
    let app = expect_context::<AppState>();
    let Quick { kind, date, desc, tags, tags_touched, acct, to, amt, err, desc_ref } = quick;
    let busy = RwSignal::new(false);

    // pre-fill the account: the last one used, else the first you own
    Effect::new(move |_| {
        let mine = app.mine();
        if !mine.iter().any(|a| a.id.to_string() == acct.get()) {
            if let Some(a) = mine.first() {
                acct.set(a.id.to_string());
            }
        }
    });
    let transfer = Memo::new(move |_| kind.get() == "transfer");
    let to_options = move || app.accounts.get().into_iter().filter(|a| a.id.to_string() != acct.get() && !a.archived).collect::<Vec<_>>();
    Effect::new(move |_| {
        if transfer.get() && !to_options().iter().any(|a| a.id.to_string() == to.get()) {
            to.set(to_options().first().map(|a| a.id.to_string()).unwrap_or_default());
        }
    });
    // typing tags by hand stops the suggestions
    Effect::new(move |prev: Option<Vec<String>>| {
        let now = tags.get();
        if prev.is_some_and(|p| p != now) && !now.is_empty() {
            tags_touched.set(true);
        }
        now
    });
    // a description you have used before brings its tags with it
    let suggest = move || {
        let d = desc.get_untracked().trim().to_lowercase();
        if d.len() < 3 || tags_touched.get_untracked() || transfer.get_untracked() {
            return;
        }
        leptos::task::spawn_local(async move {
            let f = TxFilter { q: Some(d.clone()), kinds: Some("debit,credit".into()), limit: Some(10), ..Default::default() };
            if let Ok(p) = api::transactions(&f).await {
                if let Some(m) = p.items.into_iter().find(|t| t.description.starts_with(&d)) {
                    if !tags_touched.get_untracked() && desc.get_untracked().trim().to_lowercase() == d {
                        tags.set(m.tags);
                        tags_touched.set(false);
                    }
                }
            }
        });
    };
    let placeholder = move || {
        if transfer.get() {
            if let Some(a) = app.accounts.get().into_iter().find(|a| a.id.to_string() == to.get()) {
                return auto_description(&a);
            }
        }
        "what was it".to_string()
    };

    let submit = move |()| {
        let fail = move || err.set(Some("a new transaction needs an amount above zero, a description, and a date that is not in the future".into()));
        let raw = amt.get_untracked().trim().to_string();
        let plus = raw.starts_with('+');
        let amount = match parse_minor(raw.trim_start_matches('+')) {
            Ok(a) if a > 0 => a,
            _ => return fail(),
        };
        let is_transfer = kind.get_untracked() == "transfer";
        let d = desc.get_untracked().trim().to_lowercase();
        if (!is_transfer && d.is_empty()) || date.get_untracked().is_empty() || date.get_untracked() > fmt::today() || acct.get_untracked().is_empty() || (is_transfer && to.get_untracked().is_empty()) {
            return fail();
        }
        err.set(None);
        busy.set(true);
        let acct_id = acct.get_untracked().parse::<i64>().unwrap_or(0);
        let (path, body) = if is_transfer {
            ("/transfers", serde_json::json!({"from_account_id": acct_id, "to_account_id": to.get_untracked().parse::<i64>().unwrap_or(0), "amount": format_minor(amount), "date": date.get_untracked(), "description": d, "tags": tags.get_untracked()}))
        } else {
            let k = if plus || kind.get_untracked() == "credit" { "credit" } else { "debit" };
            ("/transactions", serde_json::json!({"account_id": acct_id, "kind": k, "amount": format_minor(amount), "date": date.get_untracked(), "description": d, "tags": tags.get_untracked()}))
        };
        leptos::task::spawn_local(async move {
            let r = api::post::<serde_json::Value>(path, &body).await;
            busy.set(false);
            match r {
                Ok(_) => {
                    desc.set(String::new());
                    amt.set(String::new());
                    tags.set(Vec::new());
                    tags_touched.set(false);
                    date.set(fmt::today());
                    app.reload();
                    quick.focus();
                }
                Err(e) if e.status == 400 || e.status == 403 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let seg_input = "d-inputbar__input";
    view! {
        <InputBar label="add a transaction" on_submit=submit error=err>
            <InputSeg caption="type" flex="0 1 130px">
                <span class="d-select">
                    <select class=seg_input prop:value=move || kind.get() on:change=move |e| kind.set(event_target_value(&e))>
                        <option value="debit">"spent"</option>
                        <option value="credit">"received"</option>
                        <option value="transfer">"transfer"</option>
                    </select>
                </span>
            </InputSeg>
            <InputSeg caption="date" flex="0 1 170px">
                <input class=seg_input type="date" max=fmt::today() prop:value=move || date.get() on:change=move |e| date.set(event_target_value(&e))/>
            </InputSeg>
            <InputSeg caption="description" flex="3 1 220px">
                <input class=seg_input type="text" node_ref=desc_ref placeholder=placeholder
                    prop:value=move || desc.get() on:input=move |e| desc.set(event_target_value(&e)) on:change=move |_| suggest()/>
            </InputSeg>
            <InputSeg caption="tags" flex="3 1 240px" composite=true for_id="qa-tags">
                <TagInput value=tags suggestions=app.tags bare=true id="qa-tags" placeholder="tags"/>
            </InputSeg>
            <InputSeg caption=Signal::derive(move || if transfer.get() { "from account".to_string() } else { "account".to_string() }) flex="1 1 170px">
                <span class="d-select">
                    <select class=seg_input prop:value=move || acct.get() on:change=move |e| acct.set(event_target_value(&e))>
                        {move || app.mine().into_iter().map(|a| { let v = a.id.to_string(); let v2 = v.clone(); view! { <option value=v selected=move || acct.get() == v2>{a.name.clone()}</option> } }).collect_view()}
                    </select>
                </span>
            </InputSeg>
            {move || transfer.get().then(|| view! {
                <InputSeg caption="to account" flex="1 1 170px">
                    <span class="d-select">
                        <select class=seg_input prop:value=move || to.get() on:change=move |e| to.set(event_target_value(&e))>
                            {move || to_options().into_iter().map(|a| { let v = a.id.to_string(); let v2 = v.clone(); view! { <option value=v selected=move || to.get() == v2>{a.name.clone()}</option> } }).collect_view()}
                        </select>
                    </span>
                </InputSeg>
            })}
            <InputSeg caption="amount" num=true flex="1 1 130px">
                <input class=seg_input type="text" inputmode="decimal" placeholder=move || format!("{}0", fmt::symbol(&app.currency()))
                    prop:value=move || amt.get() on:input=move |e| amt.set(event_target_value(&e))/>
            </InputSeg>
            <InputBarSubmit busy=busy>"add"</InputBarSubmit>
        </InputBar>
    }
}

fn read_files(e: web_sys::Event) -> Vec<web_sys::File> {
    let input = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok());
    let Some(list) = input.as_ref().and_then(|i| i.files()) else { return vec![] };
    (0..list.length()).filter_map(|i| list.item(i)).collect()
}

/// The side panel: one transaction, every field. It is a tile of the layout, not a layer over it.
#[component]
pub fn TxPanel(t: Transaction) -> impl IntoView {
    let app = expect_context::<AppState>();
    let id = t.id;
    let is_transfer = t.kind == TxKind::Transfer;
    let kind = RwSignal::new(match t.kind {
        TxKind::Debit => "expense",
        TxKind::Credit => "income",
        TxKind::Transfer => "transfer",
    });
    let desc = RwSignal::new(t.description.clone());
    let amount = RwSignal::new(fmt::plain(t.amount));
    let date = RwSignal::new(t.date.clone());
    let note = RwSignal::new(t.note.clone());
    let tags = RwSignal::new(t.tags.join(", "));
    let acct = RwSignal::new(t.account_id.to_string());
    let to_name = t.counterpart_id.map(|c| app.account_name(c)).unwrap_or_default();
    let files = RwSignal::new(t.attachments.clone());
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let confirm = RwSignal::new(false);

    let save = move || {
        let a = match parse_minor(&amount.get_untracked()) {
            Ok(a) if a > 0 => a,
            _ => return err.set(Some("enter an amount above zero, a description, and a date that is not in the future".into())),
        };
        if desc.get_untracked().trim().is_empty() || date.get_untracked().is_empty() || date.get_untracked() > fmt::today() {
            return err.set(Some("enter an amount above zero, a description, and a date that is not in the future".into()));
        }
        busy.set(true);
        let mut body = serde_json::json!({"description": desc.get_untracked().trim().to_lowercase(), "amount": format_minor(a), "date": date.get_untracked(), "note": note.get_untracked(), "tags": parse_tags(&tags.get_untracked())});
        if !is_transfer {
            body["account_id"] = acct.get_untracked().parse::<i64>().unwrap_or(0).into();
            body["kind"] = if kind.get_untracked() == "income" { "credit" } else { "debit" }.into();
        }
        leptos::task::spawn_local(async move {
            let r = api::patch::<Transaction>(&format!("/transactions/{id}"), &body).await;
            busy.set(false);
            match r {
                Ok(_) => {
                    app.panel.set(None);
                    app.reload();
                }
                Err(e) if e.status == 400 || e.status == 403 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let remove = move |_| {
        leptos::task::spawn_local(async move {
            match api::delete(&format!("/transactions/{id}")).await {
                Ok(_) => {
                    confirm.set(false);
                    app.panel.set(None);
                    app.reload();
                }
                Err(e) => app.fail(&e),
            }
        });
    };
    let attach = move |e: web_sys::Event| {
        for f in read_files(e) {
            leptos::task::spawn_local(async move {
                let buf = match wasm_bindgen_futures::JsFuture::from(f.array_buffer()).await {
                    Ok(b) => js_sys::Uint8Array::new(&b),
                    Err(_) => return,
                };
                let mime = if f.type_().is_empty() { "application/octet-stream".to_string() } else { f.type_() };
                let name = js_sys::encode_uri_component(&f.name()).as_string().unwrap_or_default();
                match api::upload(&format!("/transactions/{id}/attachments?name={name}"), &mime, buf).await {
                    Ok(a) => files.update(|v| v.push(a)),
                    Err(e) if e.status == 400 => app.error(e.message),
                    Err(e) => app.fail(&e),
                }
            });
        }
    };
    let drop_file = move |fid: i64| {
        leptos::task::spawn_local(async move {
            match api::delete(&format!("/attachments/{fid}")).await {
                Ok(_) => files.update(|v| v.retain(|a| a.id != fid)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let hints = move || {
        let have = parse_tags(&tags.get());
        app.tags.get().into_iter().filter(|g| g != "transfer" && !have.contains(g)).take(8).collect::<Vec<_>>()
    };
    let seg = move |value: &'static str| {
        let disabled = is_transfer != (value == "transfer");
        view! {
            <button type="button" class="seg__opt" aria-pressed=move || (kind.get() == value).to_string() disabled=disabled on:click=move |_| kind.set(value)>{value}</button>
        }
    };
    view! {
        <form class="d-rightbar__inner" aria-labelledby="sh-t" on:submit=move |e| { e.prevent_default(); save() }>
            <header class="d-rightbar__head">
                <h2 class="d-rightbar__title" id="sh-t">"edit transaction"</h2>
                <button type="button" class="d-btn d-btn--ghost d-btn--icon d-btn--sm" aria-label="close" on:click=move |_| app.panel.set(None)><Ico d=X/></button>
            </header>
            <div class="d-rightbar__body" tabindex="0" role="group" aria-label="transaction details" style="display:flex;flex-direction:column;gap:12px;padding:16px;font-size:inherit;overscroll-behavior:contain">
                <div class="seg" role="group" aria-label="kind of transaction">{seg("expense")}{seg("income")}{seg("transfer")}</div>
                <div class="d-field">
                    <label class="d-label" for="sh-amt">"amount"</label>
                    <div class="d-input amt" style="display:flex;align-items:baseline;gap:8px;height:56px;padding:0 16px;box-sizing:border-box;width:100%">
                        <span aria-hidden="true" style="align-self:center;color:var(--text-secondary);font-size:var(--font-size-xl)">{move || fmt::symbol(&app.currency())}</span>
                        <input id="sh-amt" type="text" inputmode="decimal" placeholder="0" aria-describedby="sh-amt-c" prop:value=move || amount.get() on:input=move |e| amount.set(event_target_value(&e))
                            style="flex:1;min-width:0;align-self:stretch;padding:0;border:0;outline:none;background:none;font:inherit;font-size:var(--font-size-2xl);font-weight:500;color:var(--text-max)"/>
                        <span id="sh-amt-c" style="align-self:center;color:var(--text-secondary);font-size:var(--font-size-xs)">{move || app.currency()}</span>
                    </div>
                </div>
                <div class="d-field">
                    <label class="d-label" for="sh-desc">"description"</label>
                    <input class="d-input" id="sh-desc" type="text" prop:value=move || desc.get() on:input=move |e| desc.set(event_target_value(&e))/>
                </div>
                <div class="d-field">
                    <label class="d-label" for="sh-date">"date"</label>
                    <input class="d-input" id="sh-date" type="date" max=fmt::today() aria-describedby="sh-date-h" prop:value=move || date.get() on:change=move |e| date.set(event_target_value(&e))/>
                    <span class="d-hint" id="sh-date-h">"completed transactions only. today or earlier."</span>
                </div>
                <div class="d-field">
                    <label class="d-label" for="sh-acct">{if is_transfer { "from account" } else { "account" }}</label>
                    <span class="d-select">
                        <select class="d-input" id="sh-acct" disabled=is_transfer on:change=move |e| acct.set(event_target_value(&e))>
                            {move || app.accounts.get().into_iter().filter(|a| a.owners.iter().any(|o| o.id == app.my_id()) || a.id.to_string() == acct.get_untracked()).map(|a| { let v = a.id.to_string(); let sel = v == acct.get_untracked(); view! { <option value=v selected=sel>{a.name.clone()}</option> } }).collect_view()}
                        </select>
                    </span>
                </div>
                {is_transfer.then(|| view! {
                    <div class="d-field">
                        <label class="d-label" for="sh-to">"to account"</label>
                        <span class="d-select"><select class="d-input" id="sh-to" disabled=true aria-describedby="sh-to-h"><option>{to_name.clone()}</option></select></span>
                        <span class="d-hint" id="sh-to-h">"a transfer is not counted as spending. to change its accounts, delete it and add a new one."</span>
                    </div>
                })}
                <div class="d-field">
                    <label class="d-label" for="sh-tags">"tags"</label>
                    <input class="d-input" id="sh-tags" type="text" aria-describedby="sh-tags-h" prop:value=move || tags.get() on:input=move |e| tags.set(event_target_value(&e))/>
                    <span class="d-hint" id="sh-tags-h">"separate with commas. the first tag groups it in insights."</span>
                    <div class="d-row" role="group" aria-label="add a tag" style="gap:4px">
                        {move || hints().into_iter().map(|g| { let g2 = g.clone(); view! {
                            <button type="button" class="d-btn d-btn--ghost d-btn--sm" on:click=move |_| tags.update(|t| { let mut l = parse_tags(t); l.push(g2.clone()); *t = l.join(", "); })>{format!("+ {g}")}</button>
                        } }).collect_view()}
                    </div>
                </div>
                <div class="d-field">
                    <label class="d-label" for="sh-note">"notes "<span>"(optional)"</span></label>
                    <textarea class="d-input" id="sh-note" rows="3" aria-describedby="sh-note-h" prop:value=move || note.get() on:input=move |e| note.set(event_target_value(&e))></textarea>
                    <span class="d-hint" id="sh-note-h">"anything worth remembering: who it was for, an order number, why."</span>
                </div>
                <div class="d-field">
                    <div class="d-row" style="justify-content:space-between;flex-wrap:nowrap">
                        <span class="d-label" id="sh-att-l">"attachments"</span>
                        <label class="d-btn d-btn--ghost d-btn--icon d-btn--sm" title="attach files" style="cursor:pointer">
                            <Ico d=CLIP/>
                            <input class="d-sr" type="file" multiple=true aria-label="attach files" on:change=attach/>
                        </label>
                    </div>
                    {move || (!files.get().is_empty()).then(|| view! {
                        <div class="d-row" role="list" aria-labelledby="sh-att-l" style="gap:4px">
                            {files.get().into_iter().map(|a| {
                                let fid = a.id;
                                view! {
                                    <span class="d-badge" role="listitem" title=format!("{} kb", (a.size + 1023) / 1024) style="max-width:100%">
                                        <button type="button" on:click=move |_| { leptos::task::spawn_local(async move { if let Err(e) = api::open_attachment(fid).await { app.fail(&e) } }); }
                                            style="min-width:0;overflow:hidden;text-overflow:ellipsis;padding:0;border:0;background:none;font:inherit;color:inherit;cursor:pointer">{a.name.clone()}</button>
                                        <button type="button" class="d-badge__x" aria-label=format!("remove {}", a.name) on:click=move |_| drop_file(fid)><Ico d=X small=true/></button>
                                    </span>
                                }
                            }).collect_view()}
                        </div>
                    })}
                </div>
                {move || err.get().map(|m| view! { <span class="d-error" role="alert">{format!("error: {m}")}</span> })}
            </div>
            <footer class="d-row" style="flex:none;justify-content:space-between;flex-wrap:nowrap;gap:8px;padding:12px 16px;border-top:1px solid var(--border-default)">
                <button type="button" class="d-btn d-btn--danger d-btn--icon" aria-label="delete transaction" title="delete transaction" on:click=move |_| confirm.set(true)><Ico d=TRASH_LINES/></button>
                <button type="submit" class="d-btn d-btn--primary" title="save transaction" aria-busy=move || busy.get().then_some("true")><Ico d=CHECK/>"save"</button>
            </footer>
        </form>
        <Dialog open=confirm title="delete transaction" footer=move || view! {
            <Button on:click=move |_| confirm.set(false)>"cancel"</Button>
            <Button variant=ButtonVariant::Danger on:click=remove>"delete transaction"</Button>
        }>
            <p>{if is_transfer { "this removes both sides of the transfer. balances change back." } else { "this removes it and changes the account balance back." }}</p>
        </Dialog>
    }
}

use dots_design::prelude::*;
use leptos::prelude::*;
use tracer_api::money::parse_minor;
use tracer_api::*;
use wasm_bindgen::JsCast;

use crate::fmt;
use crate::state::AppState;
use crate::api;

const RANGES: [(&str, &str); 4] = [("7", "last 7 days"), ("30", "last 30 days"), ("month", "this month"), ("90", "last 90 days")];

fn range_bounds(sel: &[String]) -> (Option<String>, Option<String>) {
    match sel.first().map(String::as_str) {
        Some("7") => (Some(fmt::days_ago(6)), None),
        Some("30") => (Some(fmt::days_ago(29)), None),
        Some("90") => (Some(fmt::days_ago(89)), None),
        Some("month") => (Some(format!("{}-01", &fmt::today()[..7])), None),
        _ => (None, None),
    }
}

fn join(v: &[String]) -> Option<String> {
    (!v.is_empty()).then(|| v.join(","))
}

#[component]
pub fn Transactions() -> impl IntoView {
    let app = expect_context::<AppState>();

    // filters
    let q = RwSignal::new(String::new());
    let kinds = RwSignal::new(Vec::<String>::new());
    let accts = RwSignal::new(Vec::<String>::new());
    let people = RwSignal::new(Vec::<String>::new());
    let tag_sel = RwSignal::new(Vec::<String>::new());
    let range = RwSignal::new(Vec::<String>::new());
    let sort_key = RwSignal::new("date");
    let sort_dir = RwSignal::new(SortDirection::Descending);
    let page = RwSignal::new(1usize);
    let page_size = RwSignal::new(10usize);

    let base = Memo::new(move |_| {
        let (from, to) = range_bounds(&range.get());
        TxFilter {
            q: Some(q.get()).filter(|s| !s.trim().is_empty()),
            kinds: join(&kinds.get()),
            accounts: join(&accts.get()),
            tags: join(&tag_sel.get()),
            member_id: people.get().first().and_then(|p| p.parse().ok()),
            from,
            to,
            collapse_transfers: Some(true),
            sort: Some(sort_key.get().to_string()),
            dir: Some(if sort_dir.get() == SortDirection::Ascending { "asc" } else { "desc" }.into()),
            ..Default::default()
        }
    });
    // a new filter or sort starts again at page 1
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
    let total = Signal::derive(move || data.get().and_then(|r| r.ok()).map(|p| p.total as usize).unwrap_or(0));
    let any_filter = move || !q.get().is_empty() || !kinds.get().is_empty() || !accts.get().is_empty() || !people.get().is_empty() || !tag_sel.get().is_empty() || !range.get().is_empty();
    let clear_all = move |_| {
        q.set(String::new());
        for s in [kinds, accts, people, tag_sel, range] {
            s.set(Vec::new());
        }
    };

    let th = move |key: &'static str| {
        let dir = Signal::derive(move || if sort_key.get() == key { sort_dir.get() } else { SortDirection::None });
        let on = Callback::new(move |()| {
            if sort_key.get_untracked() == key {
                sort_dir.update(|d| *d = d.next());
            } else {
                sort_key.set(key);
                sort_dir.set(if key == "date" { SortDirection::Descending } else { SortDirection::Ascending });
            }
        });
        (dir, on)
    };

    let editing = RwSignal::new(None::<Transaction>);
    let acct_options = Signal::derive(move || app.accounts.get().into_iter().map(|a| FilterOption::new(a.id.to_string(), a.name.clone()).meta(a.kind.as_str())).collect::<Vec<_>>());
    let tag_options = Signal::derive(move || app.tags.get().into_iter().map(|t| FilterOption::new(t.clone(), t.clone()).cat(tag_cat(&t))).collect::<Vec<_>>());
    let people_options = Signal::derive(move || app.members().into_iter().map(|m| FilterOption::new(m.id.to_string(), m.name.clone())).collect::<Vec<_>>());
    let kind_options = Signal::stored(vec![FilterOption::new("debit", "spent").meta("money out"), FilterOption::new("credit", "received").meta("money in"), FilterOption::new("transfer", "transfer").meta("between accounts")]);
    let range_options = Signal::stored(RANGES.iter().map(|(v, l)| FilterOption::new(*v, *l)).collect::<Vec<_>>());
    let name_of = move |sel: &str, list: Vec<FilterOption>| list.into_iter().find(|o| o.value == sel).map(|o| o.label).unwrap_or_else(|| sel.to_string());

    let no_accounts = Memo::new(move |_| app.me.get().is_some() && app.accounts.with(|a| a.is_empty()));

    view! {
        <div style="display:flex;flex-direction:column;gap:16px">
            <div style="display:flex;flex-direction:column;gap:4px">
                <h1 class="heading-xl">"transactions"</h1>
                <span style="color:var(--text-secondary)">{move || format!("{} {}", total.get(), if total.get() == 1 { "transaction" } else { "transactions" })}</span>
            </div>
            {move || if no_accounts.get() {
                view! {
                    <EmptyState icon="wallet" title="no accounts yet" hint="add the accounts your money lives in, then record what moves.">
                        <ButtonLink href="/accounts" variant=ButtonVariant::Primary>"add an account"</ButtonLink>
                    </EmptyState>
                }.into_any()
            } else {
                view! {
                    <QuickAdd/>
                    <FilterBar label="filters">
                        <FilterSearch label="search transactions" value=q placeholder="search description or tag"/>
                        <Filter label="type" options=kind_options selected=kinds/>
                        <Filter label="account" options=acct_options selected=accts/>
                        {move || app.has_family().then(|| view! { <Filter label="person" options=people_options selected=people single=true empty="everyone"/> })}
                        <Filter label="tags" options=tag_options selected=tag_sel/>
                        <Filter label="date" options=range_options selected=range single=true empty="any"/>
                        {move || any_filter().then(|| view! { <Button variant=ButtonVariant::Ghost on:click=clear_all>"clear all"</Button> })}
                        {move || any_filter().then(|| view! {
                            <FilterActive>
                                {move || (!q.get().is_empty()).then(|| view! { <Tag text=format!("“{}”", q.get()) on_remove=move |()| q.set(String::new())/> })}
                                {move || kinds.get().into_iter().map(|k| { let k2 = k.clone(); view! { <Tag text=format!("type: {}", name_of(&k, vec![FilterOption::new("debit","spent"),FilterOption::new("credit","received"),FilterOption::new("transfer","transfer")])) on_remove=move |()| kinds.update(|v| v.retain(|x| *x != k2))/> } }).collect_view()}
                                {move || accts.get().into_iter().map(|a| { let a2 = a.clone(); view! { <Tag text=format!("account: {}", app.account_name(a.parse().unwrap_or(0))) on_remove=move |()| accts.update(|v| v.retain(|x| *x != a2))/> } }).collect_view()}
                                {move || people.get().into_iter().map(|p| view! { <Tag text=format!("person: {}", name_of(&p, people_options.get())) on_remove=move |()| people.set(Vec::new())/> }).collect_view()}
                                {move || tag_sel.get().into_iter().map(|t| { let t2 = t.clone(); view! { <Tag text=t on_remove=move |()| tag_sel.update(|v| v.retain(|x| *x != t2))/> } }).collect_view()}
                                {move || range.get().first().map(|r| { let l = RANGES.iter().find(|(v, _)| v == r).map(|(_, l)| *l).unwrap_or(""); view! { <Tag text=format!("date: {l}") on_remove=move |()| range.set(Vec::new())/> } })}
                            </FilterActive>
                        })}
                    </FilterBar>
                    <Suspense fallback=|| view! { <EmptyLoading title="loading transactions"/> }>
                        {move || data.get().map(|r| match r {
                            Err(e) => view! { <EmptyError title="could not load transactions" hint=e.message/> }.into_any(),
                            Ok(p) if p.items.is_empty() => view! {
                                <EmptyState icon="inbox" title=if any_filter() { "nothing matches" } else { "no transactions yet" } hint=if any_filter() { "try fewer filters." } else { "add the first one above." }/>
                            }.into_any(),
                            Ok(p) => {
                                let (date_s, date_on) = th("date");
                                let (desc_s, desc_on) = th("description");
                                let (amt_s, amt_on) = th("amount");
                                let family = app.has_family();
                                view! {
                                    <Table label="transactions. select a field to edit it in place" size=Size::Lg>
                                        <thead>
                                            <tr>
                                                <Th sort=date_s on_sort=date_on>"date"</Th>
                                                <Th sort=desc_s on_sort=desc_on>"description"</Th>
                                                <Th>"tags"</Th>
                                                <Th>"account"</Th>
                                                {family.then(|| view! { <Th>"by"</Th> })}
                                                <Th num=true sort=amt_s on_sort=amt_on>"amount"</Th>
                                                <Th><span class="d-sr">"open"</span></Th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {p.items.into_iter().map(|t| view! { <Row t=t family=family editing=editing/> }).collect_view()}
                                        </tbody>
                                    </Table>
                                }.into_any()
                            }
                        })}
                    </Suspense>
                    <Pager total=total page=page page_size=page_size sticky=true/>
                }.into_any()
            }}
        </div>
        <TxSheet editing=editing/>
    }
}

#[component]
fn Row(t: Transaction, family: bool, editing: RwSignal<Option<Transaction>>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let (id, amount) = (t.id, t.amount);
    let save = move |body: serde_json::Value| {
        leptos::task::spawn_local(async move {
            match api::patch::<Transaction>(&format!("/transactions/{id}"), &body).await {
                Ok(_) => app.reload(),
                Err(e) => app.fail(&e),
            }
        });
    };
    let desc = t.description.clone();
    let desc_shown = if desc.is_empty() { "—".to_string() } else { desc.clone() };
    let raw_amount = fmt::plain(amount);
    let open = t.clone();
    let tags_view = t.tags.clone().into_iter().map(|g| { let c = tag_cat(&g); view! { <Badge cat=c>{g}</Badge>" " } }).collect_view();
    let (by_initials, by_name) = (t.created_by.initials.clone(), t.created_by.name.clone());
    let day = fmt::day(&t.date);
    let account = app.account_name(t.account_id);
    view! {
        <Tr>
            <Td>{day}</Td>
            <EditCell label="description" value=desc on_save=move |v: String| save(serde_json::json!({"description": v}))>{desc_shown.clone()}</EditCell>
            <Td>{tags_view}</Td>
            <Td>{account}</Td>
            {family.then(|| view! { <Td><Avatar initials=by_initials name=by_name size=AvatarSize::Sm/></Td> })}
            <EditCell label="amount" kind=EditKind::Number num=true value=raw_amount on_save=move |v: String| save(serde_json::json!({"amount": v}))>{app.signed(amount)}</EditCell>
            <Td>
                <Button variant=ButtonVariant::Ghost size=Size::Sm icon=true label="open transaction" on:click=move |_| editing.set(Some(open.clone()))><Icon name="ellipsis"/></Button>
            </Td>
        </Tr>
    }
}

#[component]
fn QuickAdd() -> impl IntoView {
    let app = expect_context::<AppState>();
    let kind = RwSignal::new("debit".to_string());
    let date = RwSignal::new(fmt::today());
    let desc = RwSignal::new(String::new());
    let tags = RwSignal::new(Vec::<String>::new());
    let acct = RwSignal::new(String::new());
    let to = RwSignal::new(String::new());
    let amt = RwSignal::new(String::new());
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let desc_ref = NodeRef::<leptos::html::Input>::new();

    // pre-fill the account: the last used, else the first you own
    Effect::new(move |_| {
        let mine = app.mine();
        if acct.get_untracked().is_empty() || !mine.iter().any(|a| a.id.to_string() == acct.get_untracked()) {
            if let Some(a) = mine.first() {
                acct.set(a.id.to_string());
            }
        }
    });
    let transfer = move || kind.get() == "transfer";
    let to_options = move || app.accounts.get().into_iter().filter(|a| a.id.to_string() != acct.get() && !a.archived).collect::<Vec<_>>();
    Effect::new(move |_| {
        if transfer() && !to_options().iter().any(|a| a.id.to_string() == to.get_untracked()) {
            to.set(to_options().first().map(|a| a.id.to_string()).unwrap_or_default());
        }
    });

    let submit = move |()| {
        let amount = match parse_minor(&amt.get_untracked()) {
            Ok(a) if a > 0 => a,
            _ => return err.set(Some("a new transaction needs an amount above zero".into())),
        };
        let is_transfer = kind.get_untracked() == "transfer";
        if !is_transfer && desc.get_untracked().trim().is_empty() {
            return err.set(Some("a new transaction needs a description".into()));
        }
        if date.get_untracked() > fmt::today() {
            return err.set(Some("the date cannot be in the future".into()));
        }
        err.set(None);
        busy.set(true);
        let body = if is_transfer {
            serde_json::json!({"from_account_id": acct.get_untracked().parse::<i64>().unwrap_or(0), "to_account_id": to.get_untracked().parse::<i64>().unwrap_or(0), "amount": tracer_api::money::format_minor(amount), "date": date.get_untracked(), "description": desc.get_untracked(), "tags": tags.get_untracked()})
        } else {
            serde_json::json!({"account_id": acct.get_untracked().parse::<i64>().unwrap_or(0), "kind": kind.get_untracked(), "amount": tracer_api::money::format_minor(amount), "date": date.get_untracked(), "description": desc.get_untracked(), "tags": tags.get_untracked()})
        };
        let path = if is_transfer { "/transfers" } else { "/transactions" };
        leptos::task::spawn_local(async move {
            let r = if is_transfer { api::post::<serde_json::Value>(path, &body).await.map(|_| ()) } else { api::post::<Transaction>(path, &body).await.map(|_| ()) };
            busy.set(false);
            match r {
                Ok(()) => {
                    desc.set(String::new());
                    amt.set(String::new());
                    tags.set(Vec::new());
                    app.ok("added");
                    app.reload();
                    if let Some(i) = desc_ref.get_untracked() {
                        let _ = i.focus();
                    }
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
                <input class=seg_input type="text" node_ref=desc_ref placeholder=move || if transfer() { "optional" } else { "what was it" }
                    prop:value=move || desc.get() on:input=move |e| desc.set(event_target_value(&e))/>
            </InputSeg>
            <InputSeg caption="tags" flex="3 1 240px" composite=true for_id="qa-tags">
                <TagInput value=tags suggestions=app.tags bare=true id="qa-tags" placeholder="add tags"/>
            </InputSeg>
            <InputSeg caption=Signal::derive(move || if transfer() { "from account" } else { "account" }) flex="1 1 170px">
                <span class="d-select">
                    <select class=seg_input prop:value=move || acct.get() on:change=move |e| acct.set(event_target_value(&e))>
                        {move || app.mine().into_iter().map(|a| view! { <option value=a.id.to_string() selected=move || acct.get() == a.id.to_string()>{a.name.clone()}</option> }).collect_view()}
                    </select>
                </span>
            </InputSeg>
            {move || transfer().then(|| view! {
                <InputSeg caption="to account" flex="1 1 170px">
                    <span class="d-select">
                        <select class=seg_input prop:value=move || to.get() on:change=move |e| to.set(event_target_value(&e))>
                            {move || to_options().into_iter().map(|a| view! { <option value=a.id.to_string() selected=move || to.get() == a.id.to_string()>{a.name.clone()}</option> }).collect_view()}
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

#[component]
fn TxSheet(editing: RwSignal<Option<Transaction>>) -> impl IntoView {
    let open = RwSignal::new(false);
    Effect::new(move |_| open.set(editing.get().is_some()));
    Effect::new(move |_| {
        if !open.get() && editing.get_untracked().is_some() {
            editing.set(None);
        }
    });
    view! {
        <Sheet open=open title="transaction">
            {move || editing.get().map(|t| view! { <TxForm t=t editing=editing/> })}
        </Sheet>
    }
}

#[component]
fn TxForm(t: Transaction, editing: RwSignal<Option<Transaction>>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let id = t.id;
    let is_transfer = t.kind == TxKind::Transfer;
    let desc = RwSignal::new(t.description.clone());
    let amount = RwSignal::new(fmt::plain(t.amount));
    let date = RwSignal::new(t.date.clone());
    let note = RwSignal::new(t.note.clone());
    let tags = RwSignal::new(t.tags.clone());
    let acct = RwSignal::new(t.account_id.to_string());
    let files = RwSignal::new(t.attachments.clone());
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let confirm = RwSignal::new(false);

    let save = move |_| {
        let Ok(a) = parse_minor(&amount.get_untracked()) else { return err.set(Some("enter an amount".into())) };
        if a <= 0 {
            return err.set(Some("amount must be above zero".into()));
        }
        busy.set(true);
        let body = serde_json::json!({"description": desc.get_untracked(), "amount": tracer_api::money::format_minor(a), "date": date.get_untracked(), "note": note.get_untracked(), "tags": tags.get_untracked(),
            "account_id": if is_transfer { None } else { acct.get_untracked().parse::<i64>().ok() }});
        leptos::task::spawn_local(async move {
            let r = api::patch::<Transaction>(&format!("/transactions/{id}"), &body).await;
            busy.set(false);
            match r {
                Ok(_) => {
                    app.ok("saved");
                    editing.set(None);
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
                    app.ok("deleted");
                    confirm.set(false);
                    editing.set(None);
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
    view! {
        <form class="d-stack" style="gap:var(--space-16)" on:submit=move |e| { e.prevent_default(); save(()) }>
            <TextField label="description" value=desc/>
            <div class="d-row" style="gap:var(--space-12);align-items:flex-start">
                <div style="flex:1"><TextField label="amount" value=amount hint=Signal::derive(move || is_transfer.then(|| "a transfer changes both accounts".to_string()))/></div>
                <div style="flex:1"><TextField label="date" value=date input_type="date"/></div>
            </div>
            {(!is_transfer).then(|| view! {
                <Select label="account" value=acct options=Signal::derive(move || app.mine().into_iter().map(|a| SelectOption::new(a.id.to_string(), a.name.clone())).collect::<Vec<_>>())/>
            })}
            <TagInput label="tags" value=tags suggestions=app.tags hint="pick from the list, or type and press comma"/>
            <TextField label="note" value=note multiline=true rows=3 optional=true/>
            <div class="d-field">
                <span class="d-label">"files"</span>
                {move || files.get().into_iter().map(|a| {
                    let fid = a.id;
                    view! {
                        <div class="d-row" style="gap:var(--space-8)">
                            <button type="button" class="d-link" on:click=move |_| { leptos::task::spawn_local(async move { if let Err(e) = api::open_attachment(fid).await { app.fail(&e) } }); }>{a.name.clone()}</button>
                            <span style="color:var(--text-secondary)">{format!("{} kb", (a.size + 1023) / 1024)}</span>
                            <Button variant=ButtonVariant::Ghost size=Size::Sm icon=true label=format!("remove {}", a.name) on:click=move |_| drop_file(fid)><Icon name="x"/></Button>
                        </div>
                    }
                }).collect_view()}
                <input class="d-input" type="file" multiple=true on:change=attach aria-label="attach a file"/>
                <span class="d-hint">"receipts and statements, up to 10 mb each"</span>
            </div>
            {move || err.get().map(|m| view! { <span class="d-error" role="alert"><Icon name="octagon-alert" small=true/>{format!("error: {m}")}</span> })}
            <div class="d-row" style="justify-content:space-between">
                <Button variant=ButtonVariant::Danger on:click=move |_| confirm.set(true)>"delete transaction"</Button>
                <div class="d-row" style="gap:var(--space-8)">
                    <Button on:click=move |_| editing.set(None)>"cancel"</Button>
                    <Button variant=ButtonVariant::Primary submit=true busy=busy>"save"</Button>
                </div>
            </div>
        </form>
        <Dialog open=confirm title="delete transaction" footer=move || view! {
            <Button on:click=move |_| confirm.set(false)>"cancel"</Button>
            <Button variant=ButtonVariant::Danger on:click=remove>"delete transaction"</Button>
        }>
            <p>{if is_transfer { "this removes both sides of the transfer. balances change back." } else { "this removes it and changes the account balance back." }}</p>
        </Dialog>
    }
}

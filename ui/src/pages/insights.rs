use dots_design::prelude::*;
use leptos::prelude::*;
use tracer_api::*;

use crate::fmt;
use crate::state::AppState;
use crate::api;

#[component]
pub fn Insights() -> impl IntoView {
    let app = expect_context::<AppState>();
    let scope = RwSignal::new("all".to_string());
    let data = LocalResource::new(move || {
        let member = scope.get().parse::<i64>().ok();
        app.rev.track();
        async move { api::insights(member).await }
    });
    let chatting = RwSignal::new(false);
    view! {
        <div style="display:flex;flex-direction:column;gap:16px">
            <div class="d-row" style="justify-content:space-between;gap:12px">
                <h1 class="heading-xl">"insights"</h1>
                {move || app.has_family().then(|| view! {
                    <Tabs value=scope>
                        <TabList label="whose money">
                            <Tab value="all">"everyone"</Tab>
                            {app.members().into_iter().map(|m| view! { <Tab value=m.id.to_string()>{m.name.split(' ').next().unwrap_or("").to_string()}</Tab> }).collect_view()}
                        </TabList>
                    </Tabs>
                })}
            </div>
            <Suspense fallback=|| view! { <EmptyLoading title="loading insights"/> }>
                {move || data.get().map(|r| match r {
                    Err(e) => view! { <EmptyError title="could not load insights" hint=e.message/> }.into_any(),
                    Ok(i) if i.accounts.is_empty() => view! {
                        <EmptyState icon="wallet" title="nothing to show yet" hint="add an account and some transactions.">
                            <ButtonLink href="/accounts" variant=ButtonVariant::Primary>"add an account"</ButtonLink>
                        </EmptyState>
                    }.into_any(),
                    Ok(i) => view! { <Dashboard i=i chatting=chatting/> }.into_any(),
                })}
            </Suspense>
            <AskTracer chatting=chatting/>
        </div>
    }
}

#[component]
fn Dashboard(i: Insights, chatting: RwSignal<bool>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let worth = i.assets - i.owed;
    let kept = i.income - i.spending;
    let max_cat = i.categories.first().map(|c| c.total).unwrap_or(1).max(1);
    let spend_total = i.spending.max(1);
    let flow_max = i.months.iter().map(|m| m.income.max(m.spending)).max().unwrap_or(1).max(1);
    let day_max = i.days.iter().map(|d| d.total).max().unwrap_or(1).max(1);
    let hold = |kind: AccountKind| i.accounts.iter().filter(|a| a.kind == kind).map(|a| a.balance).sum::<i64>();
    let bars = [("bank", hold(AccountKind::Bank), false), ("investments", hold(AccountKind::Investment), false), ("cards, owed", hold(AccountKind::Credit), true), ("loans, owed", hold(AccountKind::Loan), true)];
    let bars_max = bars.iter().map(|b| b.1).max().unwrap_or(1).max(1);
    let hidden = move || if chatting.get() { Some("") } else { None };
    view! {
        <div class="d-stack" style="gap:var(--space-16)" data-hidden=hidden hidden=move || chatting.get()>
            <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:16px">
                <Stat value=app.money(worth) label="net worth"/>
                <Stat value=app.money(i.spending) label="spent, last 30 days"/>
                <Stat value=app.money(i.income) label="received, last 30 days"/>
                <Stat value=format!("{}%", if i.income > 0 { (kept * 100 / i.income).max(-999) } else { 0 }) label="kept of income"/>
            </div>
            <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(340px,1fr));gap:16px">
                <Card>
                    <CardHead title="cash flow" meta="last six months"/>
                    <CardBody>
                        <div class="d-stack" style="gap:var(--space-8)">
                            {i.months.iter().map(|m| view! {
                                <div class="d-meter-row" role="group" aria-label=format!("{}: received {}, spent {}", fmt::month(&m.month), app.money(m.income), app.money(m.spending))>
                                    <span>{fmt::month(&m.month)}</span>
                                    <div style="display:flex;flex-direction:column;gap:2px">
                                        <Meter value=m.income as f64 * 100.0 / flow_max as f64 label=format!("received {}", fmt::month(&m.month))/>
                                        <Meter value=m.spending as f64 * 100.0 / flow_max as f64 cat=2u8 label=format!("spent {}", fmt::month(&m.month))/>
                                    </div>
                                    <output>{app.money(m.income - m.spending)}</output>
                                </div>
                            }).collect_view()}
                            <span class="d-hint">"top bar received, lower bar spent, figure kept"</span>
                        </div>
                    </CardBody>
                </Card>
                <Card>
                    <CardHead title="what you own and owe"/>
                    <CardBody>
                        <div class="d-stack" style="gap:var(--space-8)">
                            {bars.iter().map(|(l, v, owed)| view! {
                                <MeterRow label=l.to_string() value=*v as f64 * 100.0 / bars_max as f64 output=app.money(*v) severity=owed.then_some(Severity::Warning)/>
                            }).collect_view()}
                            <span class="d-hint">{format!("you hold {} and owe {}", app.money(i.assets), app.money(i.owed))}</span>
                        </div>
                    </CardBody>
                </Card>
                <Card>
                    <CardHead title="where it went" meta="last 30 days"/>
                    <CardBody>
                        <div class="d-stack" style="gap:var(--space-8)">
                            {if i.categories.is_empty() { view! { <span class="d-hint">"nothing spent in this window"</span> }.into_any() } else {
                                i.categories.iter().take(8).enumerate().map(|(n, c)| view! {
                                    <MeterRow label=c.tag.clone() value=c.total as f64 * 100.0 / max_cat as f64 output=format!("{} · {}%", app.money(c.total), c.total * 100 / spend_total) cat=(n % 6) as u8 + 1/>
                                }).collect_view().into_any()
                            }}
                        </div>
                    </CardBody>
                </Card>
                <Card>
                    <CardHead title="spending rhythm" meta="last four weeks"/>
                    <CardBody>
                        <div role="img" aria-label=format!("daily spending, last four weeks, busiest day {}", app.money(day_max))
                             style="display:grid;grid-template-columns:repeat(7,1fr);gap:4px">
                            {i.days.iter().map(|d| {
                                let o = if d.total > 0 { 0.25 + 0.75 * d.total as f64 / day_max as f64 } else { 0.0 };
                                view! { <div title=format!("{}: {}", fmt::day(&d.date), if d.total > 0 { app.money(d.total) } else { "nothing spent".into() })
                                    style="height:28px;border:1px solid var(--border-default);position:relative">
                                    <div style=format!("position:absolute;inset:0;background:var(--text-strong);opacity:{o:.2}")></div>
                                </div> }
                            }).collect_view()}
                        </div>
                        <span class="d-hint">"darker is more spent. hover a day for the figure."</span>
                    </CardBody>
                </Card>
                <Card>
                    <CardHead title="coming up"/>
                    <CardBody>
                        {if i.dues.is_empty() { view! { <span class="d-hint">"nothing is due"</span> }.into_any() } else {
                            view! { <List>{i.dues.iter().map(|d| { let (meta, text) = (app.money(d.amount), format!("{} · {}", fmt::day(&d.date), d.label)); view! { <ListItem meta=meta>{text}</ListItem> } }).collect_view()}</List> }.into_any()
                        }}
                    </CardBody>
                </Card>
                <Card>
                    <CardHead title="loan runway"/>
                    <CardBody>
                        {if i.loans.is_empty() { view! { <span class="d-hint">"no loans set up"</span> }.into_any() } else {
                            i.loans.iter().filter_map(|a| a.loan.as_ref().map(|l| (a, l))).map(|(a, l)| view! {
                                <div class="d-row" style="gap:var(--space-16);align-items:center">
                                    <Ring value=l.paid as f64 * 100.0 / l.tenure.max(1) as f64 number=l.left.to_string() caption="months left" label=format!("{}, {} of {} months paid", a.name, l.paid, l.tenure)/>
                                    <div class="d-stack" style="gap:var(--space-4)">
                                        <b>{a.name.clone()}</b>
                                        <span>{format!("{} of {} paid", l.paid, l.tenure)}</span>
                                        <span class="d-hint">{format!("last emi {}", l.end)}</span>
                                        <span class="d-hint">{format!("emi {} · {} left", app.money(l.emi), app.money(l.balance))}</span>
                                    </div>
                                </div>
                            }).collect_view().into_any()
                        }}
                    </CardBody>
                </Card>
                <Card>
                    <CardHead title="investments"/>
                    <CardBody>
                        {if i.investments.is_empty() { view! { <span class="d-hint">"no investments set up"</span> }.into_any() } else {
                            view! { <List>{i.investments.iter().map(|a| {
                                let put = a.details.invested.unwrap_or(a.balance);
                                let gain = a.balance - put;
                                let (name, meta) = (a.name.clone(), format!("{} · {}{}", app.money(a.balance), if gain >= 0 { "up " } else { "down " }, app.money(gain.abs())));
                                view! { <ListItem meta=meta>{name}</ListItem> }
                            }).collect_view()}</List> }.into_any()
                        }}
                    </CardBody>
                </Card>
            </div>
        </div>
    }
}

/// One figure with its label, at a size that fits four across.
#[component]
fn Stat(#[prop(into)] value: String, #[prop(into)] label: String) -> impl IntoView {
    view! {
        <div class="d-card"><div class="d-card__body d-stack" style="gap:var(--space-4)">
            <span class="d-hint">{label}</span>
            <span class="heading-2xl">{value}</span>
        </div></div>
    }
}

#[component]
fn AskTracer(chatting: RwSignal<bool>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let messages = RwSignal::new(Vec::<ChatMessage>::new());
    let next = RwSignal::new(0u64);
    let busy = RwSignal::new(false);
    let _ = app;
    let on_send = move |q: String| {
        let id = next.get_untracked();
        next.set(id + 2);
        messages.update(|m| {
            m.push(ChatMessage { id, from: ChatFrom::Me, text: q.clone(), pending: false });
            m.push(ChatMessage { id: id + 1, from: ChatFrom::Them, text: String::new(), pending: true });
        });
        chatting.set(true);
        busy.set(true);
        leptos::task::spawn_local(async move {
            let text = match api::post::<Answer>("/ask", &serde_json::json!({"question": q})).await {
                Ok(a) => a.answer,
                Err(e) => format!("error: {}", e.message),
            };
            messages.update(|m| {
                if let Some(x) = m.iter_mut().find(|x| x.id == id + 1) {
                    x.text = text;
                    x.pending = false;
                }
            });
            busy.set(false);
        });
    };
    view! {
        <div style=move || if chatting.get() { "height:calc(100dvh - 160px);min-height:360px" } else { "height:420px" }>
            <Chat
                name="tracer" label="ask tracer" title="ask tracer"
                intro="answers come from your own transactions and accounts."
                messages=messages on_send=on_send busy=busy
                suggestions=vec!["how much did i spend on groceries?".to_string(), "when do my loans end?".to_string(), "what is due next?".to_string(), "how are my investments doing?".to_string()]
                placeholder="ask anything about your money"
            />
        </div>
        {move || chatting.get().then(|| view! {
            <Button variant=ButtonVariant::Ghost on:click=move |_| { chatting.set(false); messages.set(Vec::new()); }>"back to the numbers"</Button>
        })}
    }
}

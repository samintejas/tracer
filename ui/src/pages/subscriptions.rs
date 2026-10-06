use dots_ui::prelude::*;
use leptos::prelude::*;
use tracer_api::money::{format_minor, parse_minor};
use tracer_api::*;

use crate::api;
use crate::fmt;
use crate::icons::*;
use crate::state::{AppState, Side};

/// What a subscription costs spread over a month: a yearly one is a twelfth.
fn per_month(s: &Subscription) -> i64 {
    if s.cycle == Cycle::Yearly { s.amount / 12 } else { s.amount }
}

/// `/subscriptions`: standing charges. Each renewal is added to transactions on its date.
#[component]
pub fn Subscriptions() -> impl IntoView {
    let app = expect_context::<AppState>();
    let data = LocalResource::new(move || {
        app.rev.track();
        async move { api::subscriptions().await }
    });
    // keep the last good list on screen while the next one loads
    let shown = RwSignal::new(None::<Result<Vec<Subscription>, api::ApiError>>);
    Effect::new(move |_| {
        if let Some(r) = data.get() {
            shown.set(Some(r));
        }
    });
    let open = move |s: Subscription| app.side.set(Some(Side::Sub(Some(s))));
    let selected = move |id: i64| app.side.with(|s| matches!(s, Some(Side::Sub(Some(x))) if x.id == id)).to_string();
    view! {
        <div class="dash-wrap">
            {move || match shown.get() {
                None => view! { <EmptyLoading title="loading subscriptions"/> }.into_any(),
                Some(Err(e)) => view! { <EmptyError title="could not load subscriptions" hint=e.message/> }.into_any(),
                Some(Ok(list)) => {
                    let live: Vec<&Subscription> = list.iter().filter(|s| s.active).collect();
                    let month: i64 = live.iter().map(|s| per_month(s)).sum();
                    let paused = list.len() - live.len();
                    let summary = format!("{} active{} · renewals are added to transactions on their date", live.len(), if paused > 0 { format!(", {paused} paused") } else { String::new() });
                    let next = live.iter().filter_map(|s| s.next.clone().map(|n| (n, *s))).min_by(|a, b| a.0.cmp(&b.0));
                    let (next_value, next_note) = match &next {
                        Some((d, s)) => (fmt::day_label(d), format!("{} · {}", s.name, app.money(s.amount))),
                        None => ("none".to_string(), String::new()),
                    };
                    let stats = [
                        ("every month", app.money(month), "yearly ones spread over 12 months".to_string()),
                        ("every year", app.money(month * 12), format!("{} active", live.len())),
                        ("next renewal", next_value, next_note),
                    ];
                    let empty = list.is_empty();
                    view! {
                        <div class="d-row" style="flex:none;justify-content:space-between;gap:12px">
                            <div style="display:flex;flex-direction:column;gap:4px">
                                <h1 class="heading-xl">"subscriptions"</h1>
                                <span style="color:var(--text-secondary)">{summary}</span>
                            </div>
                            <button type="button" class="d-btn d-btn--primary" on:click=move |_| app.side.set(Some(Side::Sub(None)))><Ico d=PLUS/>"add subscription"</button>
                        </div>
                        <div style="flex:none;display:grid;grid-template-columns:repeat(auto-fit,minmax(min(180px,100%),1fr));gap:12px">
                            {stats.into_iter().map(|(label, value, note)| view! {
                                <div class="d-card"><div class="d-card__body" style="display:flex;flex-direction:column;gap:4px">
                                    <span style="color:var(--text-secondary);font-size:var(--font-size-xs)">{label}</span>
                                    <span class="heading-xl" style="color:var(--text-strong)">{value}</span>
                                    <span style="color:var(--text-secondary);font-size:var(--font-size-xs)">{note}</span>
                                </div></div>
                            }).collect_view()}
                        </div>
                        {if empty {
                            view! {
                                <EmptyState title="no subscriptions yet" hint="add the things you pay for every month or year.">
                                    <Button on:click=move |_| app.side.set(Some(Side::Sub(None)))>"add subscription"</Button>
                                </EmptyState>
                            }.into_any()
                        } else {
                            view! {
                                <div class="d-table-wrap" tabindex="0" role="region" aria-label="subscriptions. select a row to edit it" style="flex:0 1 auto;min-height:0;overscroll-behavior:contain">
                                    <table class="d-table d-table--lg">
                                        <thead><tr>
                                            <th scope="col">"name"</th><th scope="col">"billed"</th><th scope="col">"next renewal"</th>
                                            <th scope="col">"paid from"</th><th scope="col">"status"</th><th scope="col" class="d-num">"amount"</th>
                                        </tr></thead>
                                        <tbody>
                                            {list.into_iter().map(|s| {
                                                let (id, s2, s3) = (s.id, s.clone(), s.clone());
                                                view! {
                                                    <tr tabindex="0" aria-selected=move || selected(id) on:click=move |_| open(s2.clone())
                                                        on:keydown=move |e| if e.key() == "Enter" || e.key() == " " { e.prevent_default(); open(s3.clone()) }>
                                                        <td style="color:var(--text-strong)">{s.name.clone()}</td>
                                                        <td><span class="d-badge">{s.cycle.as_str()}</span></td>
                                                        <td>{if s.active { s.next.as_deref().map(fmt::day_label).unwrap_or_else(|| "none".into()) } else { "none".into() }}</td>
                                                        <td>{app.account_name(s.account_id)}</td>
                                                        <td><span class="d-status" data-state=if s.active { "on" } else { "off" } data-severity=if s.active { "success" } else { "" }>{if s.active { "active" } else { "paused" }}</span></td>
                                                        <td class="d-num" style="color:var(--text-strong)">{app.money(s.amount)}</td>
                                                    </tr>
                                                }
                                            }).collect_view()}
                                        </tbody>
                                    </table>
                                </div>
                            }.into_any()
                        }}
                    }.into_any()
                }
            }}
        </div>
    }
}

/// The side panel for one subscription, or a new one when `sub` is `None`.
#[component]
pub fn SubPanel(sub: Option<Subscription>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let id = sub.as_ref().map(|s| s.id);
    let name = RwSignal::new(sub.as_ref().map(|s| s.name.clone()).unwrap_or_default());
    let amount = RwSignal::new(sub.as_ref().map(|s| fmt::plain(s.amount)).unwrap_or_default());
    let cycle = RwSignal::new(sub.as_ref().map(|s| s.cycle).unwrap_or(Cycle::Monthly));
    let next = RwSignal::new(match &sub {
        Some(s) => s.next.clone().unwrap_or_default(),
        None => fmt::in_months(1),
    });
    let acct = RwSignal::new(match &sub {
        Some(s) => s.account_id.to_string(),
        None => app.mine().first().map(|a| a.id.to_string()).unwrap_or_default(),
    });
    let tag = RwSignal::new(sub.as_ref().map(|s| s.tag.clone()).unwrap_or_default());
    let active = RwSignal::new(sub.as_ref().map(|s| s.active).unwrap_or(true));
    let current_acct = sub.as_ref().map(|s| s.account_id);
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let confirm = RwSignal::new(false);

    let save = move || {
        let a = match parse_minor(&amount.get_untracked()) {
            Ok(a) if a > 0 && !name.get_untracked().trim().is_empty() => a,
            _ => return err.set(Some("enter a name and an amount above zero".into())),
        };
        let Ok(account_id) = acct.get_untracked().parse::<i64>() else { return err.set(Some("choose the account it is paid from".into())) };
        busy.set(true);
        let body = serde_json::json!({
            "name": name.get_untracked().trim().to_lowercase(), "amount": format_minor(a), "cycle": cycle.get_untracked().as_str(),
            "next": next.get_untracked(), "account_id": account_id, "tag": tag.get_untracked().trim().to_lowercase(), "active": active.get_untracked(),
        });
        leptos::task::spawn_local(async move {
            let r = match id {
                Some(id) => api::patch::<Subscription>(&format!("/subscriptions/{id}"), &body).await,
                None => api::post::<Subscription>("/subscriptions", &body).await,
            };
            busy.set(false);
            match r {
                Ok(_) => {
                    app.side.set(None);
                    // a renewal that was already due is now a transaction: balances moved too
                    app.reload();
                }
                Err(e) if e.status == 400 || e.status == 403 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let remove = move |_| {
        let Some(id) = id else { return };
        leptos::task::spawn_local(async move {
            match api::delete(&format!("/subscriptions/{id}")).await {
                Ok(_) => {
                    confirm.set(false);
                    app.side.set(None);
                    app.bump();
                }
                Err(e) => app.fail(&e),
            }
        });
    };
    let seg = move |value: Cycle| view! {
        <button type="button" class="seg__opt" aria-pressed=move || (cycle.get() == value).to_string() on:click=move |_| cycle.set(value)>{value.as_str()}</button>
    };
    view! {
        <form class="d-rightbar__inner" aria-labelledby="ss-t" on:submit=move |e| { e.prevent_default(); save() }>
            <header class="d-rightbar__head">
                <h2 class="d-rightbar__title" id="ss-t">{if id.is_some() { "edit subscription" } else { "add subscription" }}</h2>
                <button type="button" class="d-btn d-btn--ghost d-btn--icon d-btn--sm" aria-label="close" on:click=move |_| app.side.set(None)><Ico d=X/></button>
            </header>
            <div class="d-rightbar__body" tabindex="0" role="group" aria-label="subscription details" style="display:flex;flex-direction:column;gap:12px;padding:16px;font-size:inherit;overscroll-behavior:contain">
                <div class="d-field">
                    <label class="d-label" for="ss-name">"name"</label>
                    <input class="d-input" id="ss-name" type="text" prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e))/>
                </div>
                <div class="d-field">
                    <label class="d-label" for="ss-amt">"amount"</label>
                    <div class="d-input amt" style="display:flex;align-items:baseline;gap:8px;height:56px;padding:0 16px;box-sizing:border-box;width:100%">
                        <span aria-hidden="true" style="align-self:center;color:var(--text-secondary);font-size:var(--font-size-xl)">{move || fmt::symbol(&app.currency())}</span>
                        <input id="ss-amt" type="text" inputmode="decimal" placeholder="0" prop:value=move || amount.get() on:input=move |e| amount.set(event_target_value(&e))
                            style="flex:1;min-width:0;align-self:stretch;padding:0;border:0;outline:none;background:none;font:inherit;font-size:var(--font-size-2xl);font-weight:500;color:var(--text-max)"/>
                        <span style="align-self:center;color:var(--text-secondary);font-size:var(--font-size-xs)">{move || app.currency()}</span>
                    </div>
                </div>
                <div class="d-field">
                    <span class="d-label" id="ss-cy">"billed"</span>
                    <div class="seg" role="group" aria-labelledby="ss-cy">{seg(Cycle::Monthly)}{seg(Cycle::Yearly)}</div>
                </div>
                <div class="d-field">
                    <label class="d-label" for="ss-next">"next renewal"</label>
                    <input class="d-input" id="ss-next" type="date" aria-describedby="ss-next-h" prop:value=move || next.get() on:change=move |e| next.set(event_target_value(&e))/>
                    <span class="d-hint" id="ss-next-h">"on this date a transaction is added and the date moves on."</span>
                </div>
                <div class="d-field">
                    <label class="d-label" for="ss-acct">"paid from"</label>
                    <span class="d-select">
                        <select class="d-input" id="ss-acct" on:change=move |e| acct.set(event_target_value(&e))>
                            {move || app.accounts.get().into_iter().filter(|a| a.owners.iter().any(|o| o.id == app.my_id()) && (!a.archived || Some(a.id) == current_acct)).map(|a| {
                                let v = a.id.to_string();
                                let sel = v == acct.get_untracked();
                                view! { <option value=v selected=sel>{a.name.clone()}</option> }
                            }).collect_view()}
                        </select>
                    </span>
                </div>
                <div class="d-field">
                    <label class="d-label" for="ss-tag">"tag"</label>
                    <input class="d-input" id="ss-tag" type="text" prop:value=move || tag.get() on:input=move |e| tag.set(event_target_value(&e))/>
                </div>
                <label class="set__row" style="border-top:0"><span>"active"</span><input type="checkbox" role="switch" class="d-switch" prop:checked=move || active.get() on:change=move |e| active.set(event_target_checked(&e))/></label>
                {move || err.get().map(|m| view! { <span class="d-error" role="alert">{format!("error: {m}")}</span> })}
            </div>
            <footer class="d-row" style="flex:none;justify-content:space-between;flex-wrap:nowrap;gap:8px;padding:12px 16px;border-top:1px solid var(--border-default)">
                <span>
                    {id.map(|_| view! { <button type="button" class="d-btn d-btn--danger d-btn--icon" aria-label="delete subscription" title="delete subscription" on:click=move |_| confirm.set(true)><Ico d=TRASH_LINES/></button> })}
                </span>
                <button type="submit" class="d-btn d-btn--primary" title="save subscription" aria-busy=move || busy.get().then_some("true")><Ico d=CHECK/>"save"</button>
            </footer>
        </form>
        <Dialog open=confirm title="delete subscription" footer=move || view! {
            <Button on:click=move |_| confirm.set(false)>"cancel"</Button>
            <Button variant=ButtonVariant::Danger on:click=remove>"delete subscription"</Button>
        }>
            <p>"it stops adding transactions. the ones it already added stay."</p>
        </Dialog>
    }
}

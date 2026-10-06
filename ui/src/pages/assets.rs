use dots_design::prelude::*;
use leptos::prelude::*;
use tracer_api::money::{format_minor, parse_minor};
use tracer_api::*;

use crate::api;
use crate::fmt;
use crate::icons::*;
use crate::state::{AppState, Side};

/// `↑ ₹5,000` or `↓ ₹5,000`: worth now against what was paid. Blank when the price is unknown.
fn change(app: &AppState, value: i64, cost: i64) -> String {
    if cost == 0 {
        return String::new();
    }
    format!("{} {}", if value >= cost { "↑" } else { "↓" }, app.money((value - cost).abs()))
}

fn count(n: usize) -> String {
    format!("{n} {}", if n == 1 { "asset" } else { "assets" })
}

/// `/assets`: things owned outside any account. Their value counts towards net worth in insights.
#[component]
pub fn Assets() -> impl IntoView {
    let app = expect_context::<AppState>();
    let data = LocalResource::new(move || {
        app.rev.track();
        async move { api::assets().await }
    });
    let shown = RwSignal::new(None::<Result<Vec<Asset>, api::ApiError>>);
    Effect::new(move |_| {
        if let Some(r) = data.get() {
            shown.set(Some(r));
        }
    });
    let open = move |a: Asset| app.side.set(Some(Side::Asset(Some(a))));
    let selected = move |id: i64| app.side.with(|s| matches!(s, Some(Side::Asset(Some(x))) if x.id == id)).to_string();
    view! {
        <div class="dash-wrap">
            {move || match shown.get() {
                None => view! { <EmptyLoading title="loading assets"/> }.into_any(),
                Some(Err(e)) => view! { <EmptyError title="could not load assets" hint=e.message/> }.into_any(),
                Some(Ok(list)) => {
                    let (value, cost): (i64, i64) = list.iter().fold((0, 0), |(v, c), a| (v + a.value, c + a.cost));
                    let stats = [
                        ("worth now", app.money(value), count(list.len())),
                        ("paid", app.money(cost), "purchase prices added up".to_string()),
                        ("change", { let c = change(&app, value, cost); if c.is_empty() { app.money(0) } else { c } }, "worth now against paid".to_string()),
                    ];
                    let empty = list.is_empty();
                    let summary = format!("{} · click one to edit it", count(list.len()));
                    view! {
                        <div class="d-row" style="flex:none;justify-content:space-between;gap:12px">
                            <div style="display:flex;flex-direction:column;gap:4px">
                                <h1 class="heading-xl">"assets"</h1>
                                <span style="color:var(--text-secondary)">{summary}</span>
                            </div>
                            <button type="button" class="d-btn d-btn--primary" on:click=move |_| app.side.set(Some(Side::Asset(None)))><Ico d=PLUS/>"add asset"</button>
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
                                <EmptyState title="no assets yet" hint="add things you own outside your accounts: a home, a vehicle, gold.">
                                    <Button on:click=move |_| app.side.set(Some(Side::Asset(None)))>"add asset"</Button>
                                </EmptyState>
                            }.into_any()
                        } else {
                            view! {
                                <div class="d-table-wrap" tabindex="0" role="region" aria-label="assets. select a row to edit it" style="flex:0 1 auto;min-height:0;overscroll-behavior:contain">
                                    <table class="d-table d-table--lg">
                                        <thead><tr>
                                            <th scope="col">"name"</th><th scope="col">"kind"</th><th scope="col">"bought"</th>
                                            <th scope="col" class="d-num">"paid"</th><th scope="col" class="d-num">"value now"</th><th scope="col" class="d-num">"change"</th>
                                        </tr></thead>
                                        <tbody>
                                            {list.into_iter().map(|a| {
                                                let (id, a2, a3) = (a.id, a.clone(), a.clone());
                                                view! {
                                                    <tr tabindex="0" aria-selected=move || selected(id) on:click=move |_| open(a2.clone())
                                                        on:keydown=move |e| if e.key() == "Enter" || e.key() == " " { e.prevent_default(); open(a3.clone()) }>
                                                        <td style="color:var(--text-strong)">{a.name.clone()}</td>
                                                        <td><span class="d-badge">{a.kind.clone()}</span></td>
                                                        <td>{if a.bought.is_empty() { String::new() } else { fmt::month_year(&a.bought) }}</td>
                                                        <td class="d-num">{if a.cost > 0 { app.money(a.cost) } else { String::new() }}</td>
                                                        <td class="d-num" style="color:var(--text-strong)">{app.money(a.value)}</td>
                                                        <td class="d-num">{change(&app, a.value, a.cost)}</td>
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

/// The side panel for one asset, or a new one when `asset` is `None`.
#[component]
pub fn AssetPanel(asset: Option<Asset>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let id = asset.as_ref().map(|a| a.id);
    let name = RwSignal::new(asset.as_ref().map(|a| a.name.clone()).unwrap_or_default());
    let value = RwSignal::new(asset.as_ref().map(|a| fmt::plain(a.value)).unwrap_or_default());
    let kind = RwSignal::new(asset.as_ref().map(|a| a.kind.clone()).unwrap_or_else(|| "property".into()));
    let bought = RwSignal::new(asset.as_ref().map(|a| a.bought.clone()).unwrap_or_default());
    let cost = RwSignal::new(asset.as_ref().filter(|a| a.cost > 0).map(|a| fmt::plain(a.cost)).unwrap_or_default());
    let note = RwSignal::new(asset.as_ref().map(|a| a.note.clone()).unwrap_or_default());
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let confirm = RwSignal::new(false);

    let save = move || {
        let v = match parse_minor(&value.get_untracked()) {
            Ok(v) if v >= 0 && !name.get_untracked().trim().is_empty() && !value.get_untracked().trim().is_empty() => v,
            _ => return err.set(Some("enter a name and a value".into())),
        };
        let paid = match cost.get_untracked().trim() {
            "" => 0,
            s => match parse_minor(s) {
                Ok(c) => c.abs(),
                Err(_) => return err.set(Some("the price paid is not a number".into())),
            },
        };
        busy.set(true);
        let body = serde_json::json!({
            "name": name.get_untracked().trim().to_lowercase(), "kind": kind.get_untracked(), "bought": bought.get_untracked(),
            "cost": format_minor(paid), "value": format_minor(v), "note": note.get_untracked(),
        });
        leptos::task::spawn_local(async move {
            let r = match id {
                Some(id) => api::patch::<Asset>(&format!("/assets/{id}"), &body).await,
                None => api::post::<Asset>("/assets", &body).await,
            };
            busy.set(false);
            match r {
                Ok(_) => {
                    app.side.set(None);
                    app.bump();
                }
                Err(e) if e.status == 400 || e.status == 403 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let remove = move |_| {
        let Some(id) = id else { return };
        leptos::task::spawn_local(async move {
            match api::delete(&format!("/assets/{id}")).await {
                Ok(_) => {
                    confirm.set(false);
                    app.side.set(None);
                    app.bump();
                }
                Err(e) => app.fail(&e),
            }
        });
    };
    view! {
        <form class="d-rightbar__inner" aria-labelledby="g-t" on:submit=move |e| { e.prevent_default(); save() }>
            <header class="d-rightbar__head">
                <h2 class="d-rightbar__title" id="g-t">{if id.is_some() { "edit asset" } else { "add asset" }}</h2>
                <button type="button" class="d-btn d-btn--ghost d-btn--icon d-btn--sm" aria-label="close" on:click=move |_| app.side.set(None)><Ico d=X/></button>
            </header>
            <div class="d-rightbar__body" tabindex="0" role="group" aria-label="asset details" style="display:flex;flex-direction:column;gap:12px;padding:16px;font-size:inherit;overscroll-behavior:contain">
                <div class="d-field">
                    <label class="d-label" for="g-name">"name"</label>
                    <input class="d-input" id="g-name" type="text" prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e))/>
                </div>
                <div class="d-field">
                    <label class="d-label" for="g-val">"value now"</label>
                    <div class="d-input amt" style="display:flex;align-items:baseline;gap:8px;height:56px;padding:0 16px;box-sizing:border-box;width:100%">
                        <span aria-hidden="true" style="align-self:center;color:var(--text-secondary);font-size:var(--font-size-xl)">{move || fmt::symbol(&app.currency())}</span>
                        <input id="g-val" type="text" inputmode="decimal" placeholder="0" prop:value=move || value.get() on:input=move |e| value.set(event_target_value(&e))
                            style="flex:1;min-width:0;align-self:stretch;padding:0;border:0;outline:none;background:none;font:inherit;font-size:var(--font-size-2xl);font-weight:500;color:var(--text-max)"/>
                        <span style="align-self:center;color:var(--text-secondary);font-size:var(--font-size-xs)">{move || app.currency()}</span>
                    </div>
                </div>
                <div class="d-field">
                    <label class="d-label" for="g-kind">"kind"</label>
                    <span class="d-select">
                        <select class="d-input" id="g-kind" on:change=move |e| kind.set(event_target_value(&e))>
                            {ASSET_KINDS.into_iter().map(|k| view! { <option value=k selected=move || kind.get_untracked() == k>{k}</option> }).collect_view()}
                        </select>
                    </span>
                </div>
                <div class="d-field">
                    <label class="d-label" for="g-bought">"bought"</label>
                    <input class="d-input" id="g-bought" type="month" prop:value=move || bought.get() on:change=move |e| bought.set(event_target_value(&e))/>
                </div>
                <div class="d-field">
                    <label class="d-label" for="g-cost">"price paid"</label>
                    <input class="d-input" id="g-cost" type="text" inputmode="decimal" prop:value=move || cost.get() on:input=move |e| cost.set(event_target_value(&e))/>
                </div>
                <div class="d-field">
                    <label class="d-label" for="g-note">"notes "<span>"(optional)"</span></label>
                    <textarea class="d-input" id="g-note" rows="3" prop:value=move || note.get() on:input=move |e| note.set(event_target_value(&e))></textarea>
                </div>
                {move || err.get().map(|m| view! { <span class="d-error" role="alert">{format!("error: {m}")}</span> })}
            </div>
            <footer class="d-row" style="flex:none;justify-content:space-between;flex-wrap:nowrap;gap:8px;padding:12px 16px;border-top:1px solid var(--border-default)">
                <span>
                    {id.map(|_| view! { <button type="button" class="d-btn d-btn--danger d-btn--icon" aria-label="delete asset" title="delete asset" on:click=move |_| confirm.set(true)><Ico d=TRASH_LINES/></button> })}
                </span>
                <button type="submit" class="d-btn d-btn--primary" title="save asset" aria-busy=move || busy.get().then_some("true")><Ico d=CHECK/>"save"</button>
            </footer>
        </form>
        <Dialog open=confirm title="delete asset" footer=move || view! {
            <Button on:click=move |_| confirm.set(false)>"cancel"</Button>
            <Button variant=ButtonVariant::Danger on:click=remove>"delete asset"</Button>
        }>
            <p>"it stops counting towards what you own."</p>
        </Dialog>
    }
}

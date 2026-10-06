use dots_design::prelude::*;
use leptos::prelude::*;
use tracer_api::*;

use crate::api;
use crate::fmt;
use crate::state::AppState;

const STRIPES: &str = "repeating-linear-gradient(135deg, var(--accent-warning) 0 3px, transparent 3px 6px)";

#[component]
pub fn Insights() -> impl IntoView {
    let app = expect_context::<AppState>();
    // "all", or a member id
    let scope = RwSignal::new("all".to_string());
    let data = LocalResource::new(move || {
        let member = scope.get().parse::<i64>().ok();
        app.rev.track();
        async move { api::insights(member).await }
    });
    let scope_label = move || {
        if !app.has_family() {
            return "all accounts".to_string();
        }
        match scope.get().parse::<i64>() {
            Err(_) => app.root_name(),
            Ok(id) => format!("{}, joint accounts included", app.members().iter().find(|m| m.id == id).map(|m| m.name.split(' ').next().unwrap_or("").to_string()).unwrap_or_default()),
        }
    };
    view! {
        <div class="dash-wrap">
            <div class="d-row" style="flex:none;justify-content:space-between;gap:12px">
                <h1 class="heading-xl">"insights"</h1>
                {move || app.has_family().then(|| view! {
                    <div class="d-row" role="group" aria-label="show insights for">
                        <button type="button" class="d-btn d-btn--secondary d-btn--sm" aria-pressed=move || (scope.get() == "all").to_string() on:click=move |_| scope.set("all".into())>"family"</button>
                        {app.members().into_iter().map(|m| {
                            let (id, id2) = (m.id.to_string(), m.id.to_string());
                            view! {
                                <button type="button" class="d-btn d-btn--secondary d-btn--sm" aria-pressed=move || (scope.get() == id).to_string() on:click=move |_| scope.set(id2.clone())>
                                    {m.name.split(' ').next().unwrap_or("").to_string()}
                                </button>
                            }
                        }).collect_view()}
                    </div>
                })}
            </div>
            <Suspense fallback=|| view! { <EmptyLoading title="loading insights"/> }>
                {move || data.get().map(|r| match r {
                    Err(e) => view! { <EmptyError title="could not load insights" hint=e.message/> }.into_any(),
                    Ok(i) => view! { <Dash i=i scope_label=scope_label()/> }.into_any(),
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn Dash(i: Insights, scope_label: String) -> impl IntoView {
    let app = expect_context::<AppState>();
    let money = move |v: i64| app.money(v);

    // ---- cash flow: income and spending as lines, the gap between them shaded
    let flow_max = i.months.iter().map(|m| m.income.max(m.spending)).max().unwrap_or(0).max(1) as f64;
    let n = i.months.len().max(2);
    let xy = |vals: Vec<i64>| -> Vec<(i64, i64)> {
        vals.iter().enumerate().map(|(k, v)| ((k as f64 * 300.0 / (n - 1) as f64).round() as i64, (105.0 - *v as f64 / flow_max * 95.0).round() as i64)).collect()
    };
    let inc = xy(i.months.iter().map(|m| m.income).collect());
    let sp = xy(i.months.iter().map(|m| m.spending).collect());
    let line = |p: &[(i64, i64)]| p.iter().enumerate().map(|(k, (x, y))| format!("{} {x} {y}", if k == 0 { "M" } else { "L" })).collect::<Vec<_>>().join(" ");
    let gap = format!(
        "M {} L {} Z",
        inc.iter().map(|(x, y)| format!("{x} {y}")).collect::<Vec<_>>().join(" L "),
        sp.iter().rev().map(|(x, y)| format!("{x} {y}")).collect::<Vec<_>>().join(" L ")
    );
    let grid = inc.iter().map(|(x, _)| format!("M {x} 10 V 105")).collect::<Vec<_>>().join(" ");
    let (inc_line, sp_line) = (line(&inc), line(&sp));
    let last = i.months.last().cloned();
    let (last_label, last_kept, last_rate) = match &last {
        Some(m) => (fmt::month(&m.month), money(m.income - m.spending), fmt::pct(m.income - m.spending, m.income)),
        None => (String::new(), money(0), "0%".into()),
    };
    let (sum_in, sum_out): (i64, i64) = (i.months.iter().map(|m| m.income).sum(), i.months.iter().map(|m| m.spending).sum());
    let avg_rate = fmt::pct(sum_in - sum_out, sum_in);
    let flow_aria = i.months.iter().map(|m| format!("{}: income {}, spending {}, kept {}", fmt::month(&m.month), money(m.income), money(m.spending), money(m.income - m.spending))).collect::<Vec<_>>().join(". ");
    let month_marks = i.months.iter().map(|m| {
        let title = format!("{}: income {}, spending {}, kept {}", fmt::month(&m.month), money(m.income), money(m.spending), money(m.income - m.spending));
        (fmt::pct(m.income - m.spending, m.income), fmt::month(&m.month), title)
    }).collect::<Vec<_>>();

    // ---- what you own and owe: one bar, solid for what you hold, striped for what you owe
    let hold = |f: &dyn Fn(&Account) -> bool| i.accounts.iter().filter(|a| f(a)).map(|a| a.balance).sum::<i64>();
    let whole = i.assets + i.owed;
    let bal: Vec<(String, i64, &str, f32)> = vec![
        ("bank".to_string(), hold(&|a| a.kind == AccountKind::Bank && !a.joint), "var(--text-strong)", 1.0),
        ("joint".to_string(), hold(&|a| a.kind == AccountKind::Bank && a.joint), "var(--text-strong)", 0.75),
        ("investments".to_string(), hold(&|a| a.kind == AccountKind::Investment), "var(--text-strong)", 0.5),
        ("assets".to_string(), i.things, "var(--text-strong)", 0.3),
        ("cards, owed".to_string(), hold(&|a| a.kind == AccountKind::Credit), STRIPES, 1.0),
        ("loans, owed".to_string(), hold(&|a| a.kind == AccountKind::Loan), STRIPES, 0.6),
    ]
    .into_iter()
    .filter(|b| b.1 > 0)
    .collect();
    let bal_aria = bal.iter().map(|b| format!("{} {}", b.0, money(b.1))).collect::<Vec<_>>().join(", ");

    // ---- where it went
    let cats: Vec<(String, i64, f32)> = i.categories.iter().enumerate().map(|(k, c)| (c.tag.clone(), c.total, (1.0 - k as f32 * 0.16).max(0.18))).collect();
    let cat_aria = cats.iter().map(|c| format!("{} {}", c.0, money(c.1))).collect::<Vec<_>>().join(", ");

    // ---- spending rhythm: four weeks, monday first; days after today stay empty
    let day_max = i.days.iter().map(|d| d.total).max().unwrap_or(0).max(1) as f64;
    let today = fmt::today();
    let mut heat: Vec<(String, f64, bool)> = i.days.iter().map(|d| {
        let title = format!("{}: {}", fmt::day(&d.date), if d.total > 0 { money(d.total) } else { "nothing spent".into() });
        (title, if d.total > 0 { 0.25 + 0.75 * d.total as f64 / day_max } else { 0.0 }, d.date == today)
    }).collect();
    heat.resize(28, (String::new(), 0.0, false));
    let spend_days = i.days.iter().filter(|d| d.total > 0).count();
    let heat_note = match i.days.iter().max_by_key(|d| d.total).filter(|d| d.total > 0) {
        Some(d) => format!("biggest day: {}: {}", fmt::day(&d.date), money(d.total)),
        None => "no spending recorded".to_string(),
    };

    // ---- loan runway
    let loans: Vec<_> = i.loans.iter().filter_map(|a| a.loan.as_ref().map(|l| {
        let pct = format!("{}%", (l.paid as f64 / l.tenure.max(1) as f64 * 100.0).round());
        let emi = match a.details.emi_day {
            Some(d) => format!("emi {} on the {}", money(l.emi), fmt::ordinal(d)),
            None => format!("emi {}", money(l.emi)),
        };
        (a.name.clone(), l.left.to_string(), format!("{} left", money(l.balance)), pct, format!("{} of {} paid", l.paid, l.tenure),
         format!("started {}", a.details.start.as_deref().map(fmt::month_year).unwrap_or_default()), emi, format!("last emi {}", fmt::month_year(&l.end)))
    })).collect();
    let loan_meta = match loans.len() {
        0 => String::new(),
        1 => "1 loan".to_string(),
        n => format!("{n} loans"),
    };
    let no_loans = loans.is_empty();

    let legend = "display:grid;grid-template-columns:12px minmax(0,1fr) auto auto;gap:6px 12px;align-items:center";
    view! {
        <div class="dash">
            <section class="d-card" aria-labelledby="c-flow" style="grid-column:1 / -1">
                <header class="d-card__head"><h2 class="d-card__title" id="c-flow">"cash flow"</h2><span class="d-card__meta">{format!("6 months · {scope_label}")}</span></header>
                <div class="d-card__body" style="display:flex;flex-wrap:wrap;gap:16px 32px;align-items:stretch">
                    <div style="flex:0 1 200px;display:flex;flex-direction:column;gap:4px;justify-content:center">
                        <span style="color:var(--text-secondary)">{format!("kept in {last_label}")}</span>
                        <span class="heading-3xl" style="color:var(--text-max)">{last_kept}</span>
                        <span>{format!("{last_rate} of income · 6 month average {avg_rate}")}</span>
                        <div class="d-row" style="gap:16px;margin-top:8px;color:var(--text-secondary);font-size:var(--font-size-xs)">
                            <span class="d-row" style="gap:6px"><span aria-hidden="true" style="width:12px;height:2px;background:var(--text-strong)"></span>"income"</span>
                            <span class="d-row" style="gap:6px"><span aria-hidden="true" style="width:12px;border-top:2px dashed var(--text-secondary)"></span>"spending"</span>
                            <span class="d-row" style="gap:6px"><span aria-hidden="true" style="width:12px;height:8px;background:var(--accent-primary-subtle);border-top:1px solid var(--accent-primary-border)"></span>"kept"</span>
                        </div>
                    </div>
                    <div style="flex:1 1 420px;min-width:0;min-height:0;display:flex;flex-direction:column;gap:8px">
                        <svg viewBox="0 0 300 110" preserveAspectRatio="none" role="img" aria-label=flow_aria style="flex:1 1 0;min-height:72px;width:100%;overflow:visible">
                            <path d="M 0 105 H 300 M 0 57 H 300 M 0 10 H 300" fill="none" stroke="var(--border-default)" stroke-width="1" stroke-dasharray="2 4" vector-effect="non-scaling-stroke"></path>
                            <path d=gap fill="var(--accent-primary-subtle)"></path>
                            <path d=grid fill="none" stroke="var(--border-default)" stroke-width="1" vector-effect="non-scaling-stroke"></path>
                            <path class="ins-line" d=inc_line fill="none" stroke="var(--text-strong)" stroke-width="1.5" vector-effect="non-scaling-stroke"></path>
                            <path d=sp_line fill="none" stroke="var(--text-secondary)" stroke-width="1.5" stroke-dasharray="4 4" vector-effect="non-scaling-stroke"></path>
                        </svg>
                        <div style="display:flex;justify-content:space-between;font-size:var(--font-size-xs)">
                            {month_marks.into_iter().map(|(rate, label, title)| view! {
                                <span title=title style="display:flex;flex-direction:column;align-items:center;gap:2px"><span style="color:var(--text-strong)">{rate}</span><span style="color:var(--text-secondary)">{label}</span></span>
                            }).collect_view()}
                        </div>
                    </div>
                </div>
            </section>

            <section class="d-card" aria-labelledby="iv-nw">
                <header class="d-card__head"><h2 class="d-card__title" id="iv-nw">"what you own and owe"</h2><span class="d-card__meta">{format!("net {}", app.signed(i.assets - i.owed))}</span></header>
                <div class="d-card__body" style="display:flex;flex-direction:column;gap:12px">
                    <div role="img" aria-label=bal_aria style="display:flex;height:28px;gap:2px;background:var(--bg-inset)">
                        {bal.iter().map(|(label, v, bg, o)| view! {
                            <div class="ins-grow" title=format!("{label}: {}", money(*v)) style=format!("flex:{};min-width:2px;background:{bg};opacity:{o}", (*v / 100_000).max(1))></div>
                        }).collect_view()}
                    </div>
                    <div style="display:flex;justify-content:space-between;color:var(--text-secondary);font-size:var(--font-size-xs)"><span>{format!("own {}", money(i.assets))}</span><span>{format!("owe {}", money(i.owed))}</span></div>
                    <div style=legend>
                        {bal.iter().map(|(label, v, bg, o)| view! {
                            <div style="display:contents">
                                <span aria-hidden="true" style=format!("height:12px;background:{bg};opacity:{o}")></span>
                                <span>{label.clone()}</span>
                                <span style="color:var(--text-secondary)">{fmt::pct(*v, whole)}</span>
                                <span style="color:var(--text-strong);text-align:right">{money(*v)}</span>
                            </div>
                        }).collect_view()}
                    </div>
                </div>
            </section>

            <section class="d-card" aria-labelledby="c-cat">
                <header class="d-card__head"><h2 class="d-card__title" id="c-cat">"where it went"</h2><span class="d-card__meta">{format!("last 30 days · {}", money(i.spending))}</span></header>
                <div class="d-card__body" style="display:flex;flex-direction:column;gap:12px">
                    <div role="img" aria-label=cat_aria style="display:flex;height:28px;gap:2px;background:var(--bg-inset)">
                        {cats.iter().map(|(label, v, o)| view! {
                            <div class="ins-grow" title=format!("{label}: {}", money(*v)) style=format!("flex:{};min-width:2px;background:var(--text-strong);opacity:{o:.2}", (*v).max(1))></div>
                        }).collect_view()}
                    </div>
                    <div style=legend>
                        {cats.iter().map(|(label, v, o)| view! {
                            <div style="display:contents">
                                <span aria-hidden="true" style=format!("height:12px;background:var(--text-strong);opacity:{o:.2}")></span>
                                <span>{label.clone()}</span>
                                <span style="color:var(--text-secondary)">{fmt::pct(*v, i.spending)}</span>
                                <span style="color:var(--text-strong);text-align:right">{money(*v)}</span>
                            </div>
                        }).collect_view()}
                    </div>
                    {cats.is_empty().then(|| view! { <span style="color:var(--text-secondary)">"nothing spent in the last 30 days."</span> })}
                </div>
            </section>

            <section class="d-card" aria-labelledby="c-heat">
                <header class="d-card__head"><h2 class="d-card__title" id="c-heat">"spending rhythm"</h2><span class="d-card__meta">"last 4 weeks, by day"</span></header>
                <div class="d-card__body" style="display:flex;flex-direction:column;gap:12px">
                    <div role="img" aria-label=format!("spending by day for the last four weeks. {spend_days} days with spending.") style="display:grid;grid-template-columns:repeat(7,minmax(0,1fr));gap:4px">
                        {["m", "t", "w", "t", "f", "s", "s"].into_iter().map(|d| view! { <span style="color:var(--text-secondary);font-size:var(--font-size-xs);text-align:center">{d}</span> }).collect_view()}
                        {heat.into_iter().map(|(title, o, is_today)| view! {
                            <span title=title style=format!("position:relative;height:26px;background:var(--bg-inset);outline:{}", if is_today { "1px solid var(--border-strong)" } else { "none" })>
                                <span style=format!("position:absolute;inset:0;background:var(--accent-primary);opacity:{o:.2}")></span>
                            </span>
                        }).collect_view()}
                    </div>
                    <div class="d-row" style="justify-content:space-between;gap:12px">
                        <span>{heat_note}</span>
                        <span class="d-row" aria-hidden="true" style="gap:3px;color:var(--text-secondary);font-size:var(--font-size-xs)">
                            "less"
                            <span style="width:12px;height:12px;background:var(--bg-inset)"></span>
                            <span style="width:12px;height:12px;background:var(--accent-primary);opacity:.3"></span>
                            <span style="width:12px;height:12px;background:var(--accent-primary);opacity:.6"></span>
                            <span style="width:12px;height:12px;background:var(--accent-primary)"></span>
                            "more"
                        </span>
                    </div>
                </div>
            </section>

            <section class="d-card" aria-labelledby="iv-ln" style="grid-column:1 / -1">
                <header class="d-card__head"><h2 class="d-card__title" id="iv-ln">"loan runway"</h2><span class="d-card__meta">{loan_meta}</span></header>
                <div class="d-card__body" style="display:flex;flex-direction:column;gap:20px">
                    {loans.into_iter().map(|(name, left, balance, pct, paid, start, emi, end)| view! {
                        <div style="display:flex;flex-direction:column;gap:6px">
                            <div class="d-row" style="justify-content:space-between;gap:12px">
                                <span style="color:var(--text-strong);font-weight:500">{name}</span>
                                <span><b style="color:var(--text-max);font-weight:500">{left}</b>{format!(" months left · {balance}")}</span>
                            </div>
                            <div role="img" aria-label=paid.clone() style="position:relative;height:10px;background:repeating-linear-gradient(90deg, var(--bg-inset) 0 3px, transparent 3px 6px)">
                                <div class="ins-grow" style=format!("position:absolute;inset:0 auto 0 0;width:{pct};background:var(--text-strong)")></div>
                                <span aria-hidden="true" style=format!("position:absolute;top:-5px;bottom:-5px;left:{pct};width:2px;background:var(--accent-primary)")></span>
                            </div>
                            <div style="display:flex;justify-content:space-between;gap:12px;color:var(--text-secondary);font-size:var(--font-size-xs)">
                                <span>{start}</span><span>{format!("today · {paid} · {emi}")}</span><span>{end}</span>
                            </div>
                        </div>
                    }).collect_view()}
                    {no_loans.then(|| view! { <span style="color:var(--text-secondary)">"no loans. add one under accounts to see when it ends."</span> })}
                </div>
            </section>
        </div>
    }
}

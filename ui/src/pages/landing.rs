use dots_ui::prelude::*;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::api;
use crate::icons::*;

const HEAT: [f32; 28] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.29, 0.0, 0.34, 0.28, 0.27, 0.0, 0.75, 0.31, 0.34, 0.81, 0.0, 0.31, 0.35, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

#[component]
fn Why(d: &'static str, title: &'static str, text: &'static str) -> impl IntoView {
    view! {
        <div style="display:flex;flex-direction:column;gap:8px;padding:16px;border:1px solid var(--border-default);background:var(--bg-surface)">
            <span style="color:var(--accent-primary)"><Ico d=d large=true/></span>
            <h3 class="heading-lg" style="color:var(--text-strong)">{title}</h3>
            <p style="color:var(--text-secondary)">{text}</p>
        </div>
    }
}

#[component]
fn Runway(name: &'static str, left: &'static str, paid: &'static str, pct: u32) -> impl IntoView {
    view! {
        <div style="display:flex;flex-direction:column;gap:6px">
            <div class="d-row" style="justify-content:space-between"><span style="color:var(--text-strong)">{name}</span><span>{left}</span></div>
            <div role="img" aria-label=paid style="position:relative;height:10px;background:repeating-linear-gradient(90deg,var(--bg-inset) 0 3px,transparent 3px 6px)">
                <div style=format!("position:absolute;inset:0 auto 0 0;width:{pct}%;background:var(--text-strong)")></div>
                <span style=format!("position:absolute;top:-5px;bottom:-5px;left:{pct}%;width:2px;background:var(--accent-primary)")></span>
            </div>
        </div>
    }
}

/// `/`: the landing page. Someone already signed in goes straight to their transactions.
#[component]
pub fn Landing() -> impl IntoView {
    let nav = use_navigate();
    if api::load_token().is_some() {
        nav("/transactions", Default::default());
    }
    let stripes = "repeating-linear-gradient(135deg,var(--accent-warning) 0 3px,transparent 3px 6px)";
    view! {
        <div style="background:var(--bg-base);color:var(--text-primary);min-height:100dvh">
            <SkipLink/>
            <header class="d-appbar">
                <span style="margin-left:4px;display:flex"><Mark/></span>
                <span class="d-appbar__title">"pebblelab/fin"</span>
                <span class="d-appbar__sep" aria-hidden="true"></span>
                <nav class="d-appbar__nav" aria-label="main">
                    <a class="d-appbar__link" href="#views" rel="external">"views"</a>
                    <a class="d-appbar__link" href="#family" rel="external">"family"</a>
                    <a class="d-appbar__link" href="#features" rel="external">"features"</a>
                </nav>
                <span class="d-appbar__spacer"></span>
                <ButtonLink href="/signin" size=Size::Sm>"sign in"</ButtonLink>
            </header>
            <main id="main" tabindex="-1">
                <div class="lp-hero">
                    <section class="d-container" style="padding-block:72px;display:flex;flex-wrap:wrap;gap:48px;align-items:center">
                        <div style="flex:1 1 380px;min-width:0;display:flex;flex-direction:column;gap:24px">
                            <h1 class="heading-3xl" style="max-width:16ch;text-wrap:balance">"see where your money goes."</h1>
                            <p class="text-md" style="max-width:44ch">"one ledger for every account. shared with your family, open to your ai."</p>
                            <div class="d-row" style="gap:12px">
                                <ButtonLink href="/signup" variant=ButtonVariant::Primary size=Size::Lg>"create an account"</ButtonLink>
                                <ButtonLink href="/signin" size=Size::Lg>"sign in"</ButtonLink>
                            </div>
                        </div>
                        <section class="d-card" aria-labelledby="pv-t" style="flex:1.3 1 460px">
                            <header class="d-card__head"><h2 class="d-card__title" id="pv-t">"cash flow"</h2><span class="d-card__meta">"sample data"</span></header>
                            <div class="d-card__body" style="display:flex;flex-direction:column;gap:12px">
                                <div class="d-row" style="justify-content:space-between;align-items:baseline">
                                    <span class="heading-3xl" style="color:var(--text-max)">"₹1,45,100"</span>
                                    <span style="color:var(--text-secondary)">"kept in sep · 56% of income"</span>
                                </div>
                                <svg viewBox="0 0 300 110" preserveAspectRatio="none" role="img" aria-label="income and spending over six months, with the gap between them shaded" style="width:100%;height:200px;overflow:visible">
                                    <path d="M 0 105 H 300 M 0 57 H 300 M 0 10 H 300" fill="none" stroke="var(--border-default)" stroke-width="1" stroke-dasharray="2 4" vector-effect="non-scaling-stroke"></path>
                                    <path d="M 0 14 L 60 14 L 120 11 L 180 10 L 240 10 L 300 10 L 300 63 L 240 62 L 180 62 L 120 62 L 60 64 L 0 65 Z" fill="var(--accent-primary-subtle)"></path>
                                    <path d="M 0 14 L 60 14 L 120 11 L 180 10 L 240 10 L 300 10" fill="none" stroke="var(--text-strong)" stroke-width="1.5" vector-effect="non-scaling-stroke"></path>
                                    <path d="M 0 65 L 60 64 L 120 62 L 180 62 L 240 62 L 300 63" fill="none" stroke="var(--text-secondary)" stroke-width="1.5" stroke-dasharray="4 4" vector-effect="non-scaling-stroke"></path>
                                </svg>
                                <div style="display:flex;justify-content:space-between;color:var(--text-secondary);font-size:var(--font-size-xs)">
                                    <span>"apr"</span><span>"may"</span><span>"jun"</span><span>"jul"</span><span>"aug"</span><span>"sep"</span>
                                </div>
                            </div>
                        </section>
                    </section>
                </div>

                <section id="views" style="border-top:1px solid var(--border-default)">
                    <div class="d-container" style="padding-block:56px;display:flex;flex-direction:column;gap:24px">
                        <h2 class="heading-xl">"one screen, the whole picture"</h2>
                        <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(300px,100%),1fr));gap:12px">
                            <section class="d-card" aria-labelledby="v-own">
                                <header class="d-card__head"><h3 class="d-card__title" id="v-own">"own and owe"</h3><span class="d-card__meta">"net ₹3,65,580"</span></header>
                                <div class="d-card__body" style="display:flex;flex-direction:column;gap:12px">
                                    <div role="img" aria-label="assets in three solid segments, debts in two striped segments" style="display:flex;height:28px;gap:2px">
                                        <div style="flex:731;background:var(--text-strong)"></div>
                                        <div style="flex:213;background:var(--text-strong);opacity:.75"></div>
                                        <div style="flex:1623;background:var(--text-strong);opacity:.5"></div>
                                        <div style=format!("flex:51;min-width:2px;background:{stripes}")></div>
                                        <div style=format!("flex:1973;background:{stripes};opacity:.6")></div>
                                    </div>
                                    <div style="display:flex;justify-content:space-between;color:var(--text-secondary);font-size:var(--font-size-xs)"><span>"bank · joint · investments"</span><span>"cards · loans"</span></div>
                                </div>
                            </section>
                            <section class="d-card" aria-labelledby="v-heat">
                                <header class="d-card__head"><h3 class="d-card__title" id="v-heat">"spending rhythm"</h3><span class="d-card__meta">"4 weeks"</span></header>
                                <div class="d-card__body">
                                    <div role="img" aria-label="a calendar of four weeks, brighter on days with more spending" style="display:grid;grid-template-columns:repeat(7,minmax(0,1fr));gap:4px">
                                        {HEAT.iter().map(|o| view! {
                                            <span style="position:relative;height:22px;background:var(--bg-inset)"><span style=format!("position:absolute;inset:0;background:var(--accent-primary);opacity:{o}")></span></span>
                                        }).collect_view()}
                                    </div>
                                </div>
                            </section>
                            <section class="d-card" aria-labelledby="v-loan">
                                <header class="d-card__head"><h3 class="d-card__title" id="v-loan">"loan runway"</h3><span class="d-card__meta">"2 loans"</span></header>
                                <div class="d-card__body" style="display:flex;flex-direction:column;gap:20px">
                                    <Runway name="home loan" left="89 months left" paid="91 of 180 paid" pct=51/>
                                    <Runway name="car loan" left="23 months left" paid="37 of 60 paid" pct=62/>
                                </div>
                            </section>
                        </div>
                        <span style="color:var(--text-secondary);font-size:var(--font-size-xs)">"sample data"</span>
                    </div>
                </section>

                <section id="family" style="border-top:1px solid var(--border-default)">
                    <div class="d-container" style="padding-block:56px;display:flex;flex-direction:column;gap:24px">
                        <h2 class="heading-xl">"for you, and for your family"</h2>
                        <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(340px,100%),1fr));gap:12px">
                            <section class="d-card" aria-labelledby="w-solo">
                                <header class="d-card__head"><h3 class="d-card__title" id="w-solo">"on your own"</h3><span class="d-card__meta">"individual"</span></header>
                                <div class="d-card__body d-card__body--flush">
                                    <ul class="d-list d-list--lg">
                                        <li><span class="d-list__row">"all your accounts on one ledger"</span></li>
                                        <li><span class="d-list__row">"your own insights and loan runway"</span></li>
                                        <li><span class="d-list__row">"tags, filters and quick add"</span></li>
                                        <li><span class="d-list__row">"your own ai connection"</span></li>
                                    </ul>
                                </div>
                            </section>
                            <section class="d-card" aria-labelledby="w-fam">
                                <header class="d-card__head"><h3 class="d-card__title" id="w-fam">"as a family"</h3><span class="d-card__meta">"add it any time"</span></header>
                                <div class="d-card__body d-card__body--flush">
                                    <ul class="d-list d-list--lg">
                                        <li><span class="d-list__row">"invite with a one-time code"</span></li>
                                        <li><span class="d-list__row">"accounts are private, shared or joint"</span></li>
                                        <li><span class="d-list__row">"every transaction says who made it"</span></li>
                                        <li><span class="d-list__row">"insights per person or for everyone"</span></li>
                                    </ul>
                                </div>
                            </section>
                        </div>
                    </div>
                </section>

                <section id="features" style="border-top:1px solid var(--border-default)">
                    <div class="d-container" style="padding-block:56px;display:flex;flex-direction:column;gap:24px">
                        <h2 class="heading-xl">"built differently"</h2>
                        <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(240px,100%),1fr));gap:12px">
                            <Why d=PLUG title="works with your ai" text="connect claude code, chatgpt or any mcp client. you choose what it can read and write."/>
                            <Why d=USERS title="many people, one ledger" text="several people on the same books, each with their own sign-in."/>
                            <Why d=WALLET title="every kind of account" text="bank, joint, credit, loan and investment, each with the numbers that matter for it."/>
                            <Why d=BOLT title="fast to keep up" text="one bar to add a transaction. click any cell to fix it."/>
                        </div>
                    </div>
                </section>

                <section style="border-top:1px solid var(--border-default)">
                    <div class="d-container" style="padding-block:64px;display:flex;flex-wrap:wrap;gap:24px;align-items:center;justify-content:space-between">
                        <h2 class="heading-2xl" style="max-width:24ch">"start with one account."</h2>
                        <ButtonLink href="/signup" variant=ButtonVariant::Primary size=Size::Lg>"create an account"</ButtonLink>
                    </div>
                </section>
            </main>
            <footer style="border-top:1px solid var(--border-default)">
                <div class="d-container d-row" style="padding-block:16px;justify-content:space-between;font-size:var(--font-size-sm);color:var(--text-secondary)">
                    <span>"pebblelab/fin"</span>
                    <span>"a pebblelab.in app"</span>
                </div>
            </footer>
        </div>
    }
}

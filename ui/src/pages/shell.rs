use dots_design::prelude::*;
use leptos::prelude::*;
use leptos_router::components::Outlet;
use leptos_router::hooks::{use_location, use_navigate};

use crate::api;
use crate::icons::*;
use crate::pages::transactions::TxPanel;
use crate::pages::{AssetPanel, SubPanel};
use crate::state::{AppState, Side};

/// `/accounts/3` and `settings/family` style links from a notification.
fn link_path(link: &str) -> Option<String> {
    (!link.is_empty()).then(|| format!("/{}", link.trim_start_matches('/')))
}

/// `2026-10-05 12:30:00` (utc) to `today`, `yesterday` or `05 oct`.
fn when(created: &str) -> String {
    let day = created.get(..10).unwrap_or(created);
    if day == crate::fmt::today() {
        "today".into()
    } else if day == crate::fmt::days_ago(1) {
        "yesterday".into()
    } else {
        crate::fmt::day(day)
    }
}

/// The signed-in frame: sidebar, app bar, the page, and the side panel. Redirects to sign in without a
/// token, and loads the shared data once.
#[component]
pub fn AppShell() -> impl IntoView {
    let app = AppState::new();
    provide_context(app);
    let nav = use_navigate();
    if api::load_token().is_none() {
        nav("/signin", Default::default());
    } else {
        app.reload();
    }
    {
        let nav = nav.clone();
        Effect::new(move |_| {
            if app.me.get().is_none() && api::token().is_none() {
                nav("/signin", Default::default());
            }
        });
    }
    let shell = ShellState::new();
    // the side panel is open exactly while a transaction, subscription or asset is
    Effect::new(move |_| shell.set(Region::Right, app.panel.get().is_some() || app.side.get().is_some()));

    let path = use_location().pathname;
    let page = Memo::new(move |_| path.get().trim_start_matches('/').split('/').next().unwrap_or("").to_string());
    // leaving a screen closes its panel
    Effect::new(move |_| {
        let p = page.get();
        if p != "transactions" {
            app.panel.set(None);
        }
        let keep = app.side.with_untracked(|s| match s {
            Some(Side::Sub(_)) => p == "subscriptions",
            Some(Side::Asset(_)) => p == "assets",
            None => true,
        });
        if !keep {
            app.side.set(None);
        }
    });
    let crumb = move || {
        let p = path.get();
        let parts: Vec<&str> = p.trim_start_matches('/').split('/').collect();
        match parts.as_slice() {
            ["settings"] => "settings / profile".to_string(),
            ["settings", "prefs"] => "settings / preferences".to_string(),
            ["settings", tab] => format!("settings / {tab}"),
            ["accounts", "new"] => "accounts / new".to_string(),
            ["accounts", id] => format!("accounts / {}", id.parse().map(|i| app.account_name(i)).unwrap_or_default()),
            [one] => one.to_string(),
            _ => String::new(),
        }
    };

    let theme = use_theme();
    let theme_name = RwSignal::new(theme.get_untracked().as_str().to_string());
    Effect::new(move |_| {
        let t = Theme::parse(&theme_name.get()).unwrap_or_default();
        if theme.get_untracked() != t {
            theme.set(t);
        }
    });
    Effect::new(move |_| theme_name.set(theme.get().as_str().to_string()));

    let sign_out = move |()| {
        leptos::task::spawn_local(async move {
            let _ = api::post::<serde_json::Value>("/auth/signout", &serde_json::json!({})).await;
            api::clear_token();
            app.me.set(None);
        });
    };
    let tile_name = move || app.me.with(|m| match m {
        Some(m) => m.family.as_ref().map(|f| f.name.clone()).unwrap_or_else(|| m.user.name.clone()),
        None => String::new(),
    });
    let picture = Signal::derive(move || app.me.with(|m| m.as_ref().and_then(|m| m.user.picture.clone())));
    let acct_count = move || app.accounts.with(|a| a.iter().filter(|a| !a.archived).count());
    let go_tx = { let nav = nav.clone(); move |_| nav("/transactions", Default::default()) };

    view! {
        <SkipLink/>
        <Shell state=shell height="100dvh">
            <Sidebar>
                <SidebarHead>
                    <button type="button" class="d-sidebar__tile" aria-label="tracer/fin, go to transactions" on:click=go_tx>
                        <Mark/>
                        <span class="d-sidebar__who"><b>"tracer/fin"</b><small>{tile_name}</small></span>
                    </button>
                </SidebarHead>
                <SidebarContent>
                    <SidebarGroup label="money">
                        <SidebarItem href="/transactions" icon="arrow-left-right" label="transactions" current=Signal::derive(move || page.get() == "transactions")/>
                        <SidebarItem href="/insights" icon="layout-dashboard" label="insights" current=Signal::derive(move || page.get() == "insights")/>
                        {move || view! { <SidebarItem href="/accounts" icon="wallet" label="accounts" current=Signal::derive(move || page.get() == "accounts") count=acct_count().to_string()/> }}
                    </SidebarGroup>
                    <SidebarGroup label="track">
                        <TrackItem href="/subscriptions" icon=REPEAT label="subscriptions" current=Signal::derive(move || page.get() == "subscriptions")/>
                        <TrackItem href="/assets" icon=PACKAGE label="assets" current=Signal::derive(move || page.get() == "assets")/>
                    </SidebarGroup>
                </SidebarContent>
                <SidebarFoot>
                    <Menu
                        label="account" match_width=true
                        trigger=move || view! {
                            <button type="button" class="d-sidebar__tile" aria-label=move || format!("account: {}", app.me.with(|m| m.as_ref().map(|m| m.user.name.clone()).unwrap_or_default()))>
                                <span class="d-avatar" aria-hidden="true"><Face picture=picture/></span>
                                <span class="d-sidebar__who">
                                    <b>{move || app.me.with(|m| m.as_ref().map(|m| m.user.name.clone()).unwrap_or_default())}</b>
                                    <small>{move || app.me.with(|m| m.as_ref().map(|m| m.user.email.clone()).unwrap_or_default())}</small>
                                </span>
                                <Ico d=UP_DOWN/>
                            </button>
                        }
                    >
                        <MenuLink href="/settings">"settings"</MenuLink>
                        <MenuLink href="/settings/family">"family"</MenuLink>
                        <MenuSeparator/>
                        <MenuRadioGroup label="theme" value=theme_name>
                            <MenuRadio value="dark">"dark"</MenuRadio>
                            <MenuRadio value="light">"light"</MenuRadio>
                        </MenuRadioGroup>
                        <MenuSeparator/>
                        <MenuItem on_select=sign_out>"sign out"</MenuItem>
                    </Menu>
                </SidebarFoot>
            </Sidebar>
            <ShellMain>
                <AppBar>
                    <SidebarToggle/>
                    <AppBarSep/>
                    <Breadcrumbs>
                        <Crumb href="/insights">{move || app.root_name()}</Crumb>
                        <CrumbCurrent>{crumb}</CrumbCurrent>
                    </Breadcrumbs>
                    <AppBarSpacer/>
                    <Notifications/>
                </AppBar>
                <Main><div class="page"><Outlet/></div></Main>
            </ShellMain>
            <aside class="d-rightbar" aria-label="details" inert=move || app.panel.get().is_none() && app.side.get().is_none()>
                {move || app.panel.get().map(|t| view! { <TxPanel t=t/> })}
                {move || app.side.get().map(|s| match s {
                    Side::Sub(x) => view! { <SubPanel sub=x/> }.into_any(),
                    Side::Asset(x) => view! { <AssetPanel asset=x/> }.into_any(),
                })}
            </aside>
        </Shell>
    }
}

/// A sidebar row for the screens whose icon is not in the bundled set: the same markup as `SidebarItem`.
#[component]
fn TrackItem(href: &'static str, icon: &'static str, label: &'static str, #[prop(into)] current: Signal<bool>) -> impl IntoView {
    let shell = use_shell();
    view! {
        <li class="d-sidebar__item">
            <a class="d-sidebar__btn" href=href aria-current=move || current.get().then_some("page") data-tooltip=move || shell.icon_rail().then_some(label)>
                <Ico d=icon/>
                <span class="d-sidebar__label">{label}</span>
            </a>
        </li>
    }
}

/// The bell and its panel: a title, one line of detail and when, with a dot on what is unread.
#[component]
fn Notifications() -> impl IntoView {
    let app = expect_context::<AppState>();
    let open = RwSignal::new(false);
    let nav = use_navigate();
    let unread = move || app.notes.with(|n| n.iter().filter(|n| !n.read).count());
    let refresh = move || {
        leptos::task::spawn_local(async move {
            if let Ok(n) = api::notifications().await {
                app.notes.set(n);
            }
        });
    };
    let read = move |id: Option<i64>| {
        leptos::task::spawn_local(async move {
            let _ = api::post::<serde_json::Value>("/notifications/read", &serde_json::json!({ "id": id })).await;
            refresh();
        });
    };
    let root = NodeRef::<leptos::html::Div>::new();
    // a click anywhere else, or esc, closes it
    let click = window_event_listener(leptos::ev::pointerdown, move |e| {
        use wasm_bindgen::JsCast;
        let inside = match (root.get_untracked(), e.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok())) {
            (Some(r), Some(t)) => r.contains(Some(&t)),
            _ => false,
        };
        if !inside && open.get_untracked() {
            open.set(false);
        }
    });
    on_cleanup(move || click.remove());
    let label = move || match unread() {
        0 => "notifications".to_string(),
        n => format!("notifications, {n} unread"),
    };
    let nav2 = nav.clone();
    view! {
        <div class="d-appbar__actions" style="position:relative;margin-right:8px" node_ref=root on:keydown=move |e| if e.key() == "Escape" { open.set(false) }>
            <button type="button" class="d-btn d-btn--ghost d-btn--icon d-btn--sm" aria-label=label aria-haspopup="true"
                aria-expanded=move || open.get().to_string() on:click=move |_| open.update(|o| *o = !*o)>
                <Ico d=BELL/>
                {move || { let n = unread(); (n > 0).then(|| view! { <span class="d-btn__count" aria-hidden="true">{if n > 9 { "9+".to_string() } else { n.to_string() }}</span> }) }}
            </button>
            {move || open.get().then(|| {
                let (nav, nav2) = (nav.clone(), nav2.clone());
                view! {
                    <section class="d-menu" aria-label="notifications" style="position:absolute;top:calc(100% + 8px);right:0;z-index:200;width:360px;max-width:calc(100vw - 32px);padding:0;overflow:hidden">
                        <div class="d-row" style="justify-content:space-between;flex-wrap:nowrap;min-height:44px;padding:8px 8px 8px 16px">
                            <span style="color:var(--text-strong);font-weight:500">"notifications"</span>
                            {move || (unread() > 0).then(|| view! { <button type="button" class="d-btn d-btn--ghost d-btn--sm" on:click=move |_| read(None)>"mark all read"</button> })}
                        </div>
                        <div style="max-height:min(420px, 60vh);overflow-y:auto">
                        {move || {
                            let list = app.notes.get();
                            if list.is_empty() {
                                return view! { <div style="padding:12px 16px;border-top:1px solid var(--border-default);color:var(--text-secondary)">"nothing yet. card due dates, emis and family activity show up here."</div> }.into_any();
                            }
                            let nav = nav.clone();
                            list.into_iter().map(|n| {
                                let nav = nav.clone();
                                let (id, link, unread) = (n.id, link_path(&n.link), !n.read);
                                view! {
                                    <button type="button" class="nt" on:click=move |_| {
                                        if unread { read(Some(id)); }
                                        open.set(false);
                                        if let Some(l) = &link { nav(l, Default::default()); }
                                    }>
                                        <span class="nt__dot" aria-hidden="true" style=if unread { "background:var(--accent-primary)" } else { "background:transparent" }></span>
                                        <span style=if unread { "color:var(--text-strong)" } else { "color:var(--text-primary)" }>{n.title.clone()}{unread.then(|| view! { <span class="d-sr">", unread"</span> })}</span>
                                        <span style="color:var(--text-secondary);font-size:var(--font-size-xs)">{when(&n.created_at)}</span>
                                        <span></span>
                                        <span style="grid-column:2 / 4;color:var(--text-secondary)">{n.body.clone()}</span>
                                    </button>
                                }
                            }).collect_view().into_any()
                        }}
                        </div>
                        <div class="d-row" style="justify-content:flex-end;flex-wrap:nowrap;padding:8px 8px 8px 16px;border-top:1px solid var(--border-default)">
                            <button type="button" class="d-btn d-btn--ghost d-btn--sm" on:click=move |_| { open.set(false); nav2("/settings/prefs", Default::default()); }>"notification settings"</button>
                        </div>
                    </section>
                }
            })}
        </div>
    }
}

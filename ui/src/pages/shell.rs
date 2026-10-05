use dots_design::prelude::*;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::api;
use crate::state::AppState;

fn title(page: &str) -> &'static str {
    match page {
        "transactions" => "transactions",
        "insights" => "insights",
        "accounts" => "accounts",
        _ => "profile",
    }
}

/// The signed-in frame: sidebar, app bar, content. Redirects to sign in without a token, and loads the
/// shared data once.
#[component]
pub fn AppShell(page: &'static str, children: Children) -> impl IntoView {
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
    let scope = RwSignal::new("me".to_string());
    let theme = use_theme();
    let theme_name = RwSignal::new(theme.get_untracked().as_str().to_string());
    Effect::new(move |_| {
        let t = Theme::parse(&theme_name.get()).unwrap_or_default();
        if theme.get_untracked() != t {
            theme.set(t);
        }
    });

    let switcher = move || {
        let items = match app.me.get() {
            Some(m) => match m.family {
                Some(f) => vec![SwitcherItem::new("me", f.name.clone(), f.name.chars().take(2).collect::<String>(), format!("{} {}", f.members.len(), if f.members.len() == 1 { "member" } else { "members" }))],
                None => vec![SwitcherItem::new("me", m.user.name.clone(), m.user.initials.clone(), "personal")],
            },
            None => vec![SwitcherItem::new("me", "tracer/fin", "tf", "money")],
        };
        view! { <SidebarSwitcher label="workspace" value=scope items=items/> }
    };
    let unread = move || app.notes.with(|n| n.iter().filter(|n| !n.read).count() as u32);
    let sign_out = move |()| {
        leptos::task::spawn_local(async move {
            let _ = api::post::<serde_json::Value>("/auth/signout", &serde_json::json!({})).await;
            api::clear_token();
            app.me.set(None);
        });
    };
    let acct_count = move || app.accounts.with(|a| a.len().to_string());
    view! {
        <SkipLink/>
        <Shell persist="tracer:shell" height="100dvh">
            <Sidebar>
                <SidebarHead>{switcher}</SidebarHead>
                <SidebarContent>
                    <SidebarGroup label="money">
                        <SidebarItem href="/transactions" icon="arrow-left-right" label="transactions" current=page == "transactions"/>
                        <SidebarItem href="/insights" icon="layout-dashboard" label="insights" current=page == "insights"/>
                        {move || view! { <SidebarItem href="/accounts" icon="wallet" label="accounts" current=page == "accounts" count=acct_count()/> }}
                    </SidebarGroup>
                </SidebarContent>
                <SidebarFoot>
                    {move || {
                        let (name, email, initials) = app.me.with(|m| m.as_ref().map(|m| (m.user.name.clone(), m.user.email.clone(), m.user.initials.clone())).unwrap_or_default());
                        view! {
                            <SidebarAccount label="account" initials=initials name=name meta=email>
                                <MenuLink href="/profile">"profile"</MenuLink>
                                <MenuSeparator/>
                                <MenuRadioGroup label="theme" value=theme_name>
                                    <MenuRadio value="dark">"dark"</MenuRadio>
                                    <MenuRadio value="light">"light"</MenuRadio>
                                </MenuRadioGroup>
                                <MenuSeparator/>
                                <MenuItem on_select=sign_out>"sign out"</MenuItem>
                            </SidebarAccount>
                        }
                    }}
                </SidebarFoot>
            </Sidebar>
            <ShellMain>
                <AppBar>
                    <SidebarToggle/>
                    <AppBarSep/>
                    <Breadcrumbs>
                        <Crumb href="/transactions">"tracer/fin"</Crumb>
                        <CrumbCurrent>{title(page)}</CrumbCurrent>
                    </Breadcrumbs>
                    <AppBarSpacer/>
                    <AppBarActions>
                        <Notifications unread=Signal::derive(unread)/>
                    </AppBarActions>
                </AppBar>
                <Main>
                    <div class="d-container" style="padding:var(--space-16)">{children()}</div>
                </Main>
            </ShellMain>
        </Shell>
    }
}

#[component]
fn Notifications(unread: Signal<u32>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let read_all = move |()| {
        leptos::task::spawn_local(async move {
            let _ = api::post::<serde_json::Value>("/notifications/read", &serde_json::json!({})).await;
            if let Ok(n) = api::notifications().await {
                app.notes.set(n);
            }
        });
    };
    let label = move || if unread.get() == 0 { "notifications".to_string() } else { format!("notifications, {} unread", unread.get()) };
    view! {
        <Menu
            label="notifications"
            trigger=move || view! {
                <Button variant=ButtonVariant::Ghost icon=true label=Signal::derive(label) count=unread><Icon name="bell"/></Button>
            }
        >
            <MenuLabel>"notifications"</MenuLabel>
            {move || {
                let list = app.notes.get();
                if list.is_empty() {
                    view! { <div class="d-menu__label" role="presentation">"nothing yet"</div> }.into_any()
                } else {
                    list.into_iter().take(8).map(|n| view! {
                        <MenuItem meta=n.created_at.get(..10).unwrap_or("").to_string()>{if n.read { n.text.clone() } else { format!("● {}", n.text) }}</MenuItem>
                    }).collect_view().into_any()
                }
            }}
            <MenuSeparator/>
            <MenuItem on_select=read_all>"mark all read"</MenuItem>
        </Menu>
    }
}

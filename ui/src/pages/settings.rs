use dots_ui::prelude::*;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use pebblelab_api::*;
use wasm_bindgen::JsCast;

use crate::api;
use crate::icons::*;
use crate::state::AppState;

const TABS: [(&str, &str, &str, &str); 5] = [
    ("profile", "profile", USER, "your name, picture and how to reach you."),
    ("prefs", "preferences", SLIDERS, "how pebblelab/fin looks and what it tells you about."),
    ("security", "security", LOCK, "password, sessions and deleting your account."),
    ("family", "family", USERS, "optional. share accounts and hold joint ones."),
    ("connectors", "connectors", PLUG, "give claude code, chatgpt or any mcp client its own token, with its own permissions."),
];

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn copy(app: AppState, text: String) {
    if let Some(w) = web_sys::window() {
        let _ = w.navigator().clipboard().write_text(&text);
        app.ok("copied");
    }
}

#[component]
pub fn Settings() -> impl IntoView {
    let params = use_params_map();
    let tab = Memo::new(move |_| {
        let t = params.with(|p| p.get("tab")).unwrap_or_default();
        TABS.iter().find(|x| x.0 == t).map(|x| x.0).unwrap_or("profile")
    });
    let def = move || *TABS.iter().find(|x| x.0 == tab.get()).unwrap_or(&TABS[0]);
    view! {
        <div class="set">
            <div class="set__nav">
                <h1 class="heading-xl" style="padding:0 12px 12px">"settings"</h1>
                <div role="tablist" aria-label="settings" aria-orientation="vertical" style="display:contents">
                    {TABS.iter().map(|(key, label, icon, _)| view! {
                        <a class="set__tab" role="tab" href=format!("/settings/{key}") aria-selected=move || (tab.get() == *key).to_string()><Ico d=icon/>{*label}</a>
                    }).collect_view()}
                </div>
            </div>
            <div class="set__panel" role="tabpanel" aria-label=move || def().1>
                <div style="display:flex;flex-direction:column;gap:4px">
                    <h2 class="heading-lg" style="color:var(--text-strong)">{move || def().1}</h2>
                    <span style="color:var(--text-secondary)">{move || def().3}</span>
                </div>
                {move || match tab.get() {
                    "prefs" => view! { <Prefs/> }.into_any(),
                    "security" => view! { <Security/> }.into_any(),
                    "family" => view! { <FamilyTab/> }.into_any(),
                    "connectors" => view! { <Connectors/> }.into_any(),
                    _ => view! { <ProfileTab/> }.into_any(),
                }}
            </div>
        </div>
    }
}

/// Save part of the profile and refresh what the rest of the app knows.
fn patch_me(app: AppState, body: serde_json::Value, done: impl Fn(Result<(), String>) + 'static) {
    leptos::task::spawn_local(async move {
        match api::patch::<User>("/me", &body).await {
            Ok(u) => {
                app.me.update(|m| {
                    if let Some(m) = m {
                        m.user = u;
                    }
                });
                done(Ok(()));
            }
            Err(e) if e.status == 400 || e.status == 409 => done(Err(e.message)),
            Err(e) => app.fail(&e),
        }
    });
}

#[component]
fn ProfileTab() -> impl IntoView {
    let app = expect_context::<AppState>();
    let (name, initials, email, phone) = (RwSignal::new(String::new()), RwSignal::new(String::new()), RwSignal::new(String::new()), RwSignal::new(String::new()));
    let filled = RwSignal::new(false);
    // the profile may arrive after this tab opens
    Effect::new(move |_| {
        if let (Some(u), false) = (app.me.get().map(|m| m.user), filled.get_untracked()) {
            name.set(u.name);
            initials.set(u.initials);
            email.set(u.email);
            phone.set(u.phone);
            filled.set(true);
        }
    });
    let err = RwSignal::new(None::<String>);
    let picture = Signal::derive(move || app.me.with(|m| m.as_ref().and_then(|m| m.user.picture.clone())));
    let save = move || {
        err.set(None);
        let body = serde_json::json!({"name": name.get_untracked(), "initials": initials.get_untracked(), "email": email.get_untracked(), "phone": phone.get_untracked()});
        patch_me(app, body, move |r| match r {
            Ok(()) => app.ok("saved"),
            Err(m) => err.set(Some(m)),
        });
    };
    let on_pic = move |e: web_sys::Event| {
        let input = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok());
        let Some(file) = input.and_then(|i| i.files()).and_then(|f| f.item(0)) else { return };
        if file.size() > 250_000.0 {
            return app.error("the picture is too large: use one under 250 kb");
        }
        leptos::task::spawn_local(async move {
            let Ok(buf) = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await else { return };
            let bytes = js_sys::Uint8Array::new(&buf).to_vec();
            let url = format!("data:{};base64,{}", if file.type_().is_empty() { "image/png".to_string() } else { file.type_() }, base64(&bytes));
            patch_me(app, serde_json::json!({ "picture": url }), move |r| match r {
                Ok(()) => app.ok("picture saved"),
                Err(m) => app.error(m),
            });
        });
    };
    let upload_label = move || if picture.get().is_some() { "change picture" } else { "upload picture" };
    view! {
        <form class="d-card" style="max-width:760px" on:submit=move |e| { e.prevent_default(); save() }>
            <div class="d-card__body d-row" style="gap:16px;border-bottom:1px solid var(--border-default)">
                <span class="d-avatar" role="img" aria-label="your profile picture" style="width:64px;height:64px;font-size:var(--font-size-lg);border-radius:9999px"><Face picture=picture/></span>
                <div style="display:flex;flex-direction:column;gap:8px;min-width:0">
                    <span class="d-label">"profile picture"</span>
                    <div class="d-row">
                        <label class="d-btn d-btn--secondary" style="position:relative">
                            {upload_label}
                            <input type="file" accept="image/*" aria-label=upload_label on:change=on_pic style="position:absolute;inset:0;width:100%;opacity:0;cursor:pointer"/>
                        </label>
                        {move || picture.get().is_some().then(|| view! {
                            <button type="button" class="d-btn d-btn--ghost d-btn--icon" aria-label="remove picture" title="remove picture"
                                on:click=move |_| patch_me(app, serde_json::json!({ "picture": "" }), |_| ())><Ico d=TRASH/></button>
                        })}
                    </div>
                </div>
            </div>
            <div class="d-card__body" style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(260px,100%),1fr));gap:16px;align-items:start">
                <div class="d-field">
                    <label class="d-label" for="pf-name">"name"</label>
                    <input class="d-input" id="pf-name" type="text" autocomplete="name" prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e))/>
                </div>
                <div class="d-field">
                    <label class="d-label" for="pf-initials">"initials"</label>
                    <input class="d-input" id="pf-initials" type="text" maxlength="2" aria-describedby="pf-initials-h" prop:value=move || initials.get() on:input=move |e| initials.set(event_target_value(&e))/>
                    <span class="d-hint" id="pf-initials-h">"two letters, shown on your avatar"</span>
                </div>
                <div class="d-field">
                    <label class="d-label" for="pf-email">"email"</label>
                    <input class="d-input" id="pf-email" type="email" autocomplete="email" prop:value=move || email.get() on:input=move |e| email.set(event_target_value(&e))/>
                </div>
                <div class="d-field">
                    <label class="d-label" for="pf-phone">"phone "<span>"(optional)"</span></label>
                    <input class="d-input" id="pf-phone" type="tel" autocomplete="tel" prop:value=move || phone.get() on:input=move |e| phone.set(event_target_value(&e))/>
                </div>
            </div>
            {move || err.get().map(|m| view! { <div class="d-card__body" style="padding-top:0"><span class="d-error" role="alert">{format!("error: {m}")}</span></div> })}
            <footer class="d-card__foot">
                <button type="submit" class="d-btn d-btn--primary"><Ico d=CHECK/>"save"</button>
            </footer>
        </form>
    }
}

#[component]
fn Prefs() -> impl IntoView {
    let app = expect_context::<AppState>();
    let theme = use_theme();
    let user = move || app.me.get().map(|m| m.user);
    let toggle = move |key: &'static str, on: bool| patch_me(app, serde_json::json!({ key: on }), move |r| if let Err(m) = r { app.error(m) });
    let switch = move |label: &'static str, key: &'static str, get: fn(&User) -> bool| view! {
        <label class="set__row"><span>{label}</span>
            <input type="checkbox" role="switch" class="d-switch" prop:checked=move || user().is_some_and(|u| get(&u)) on:change=move |e| toggle(key, event_target_checked(&e))/>
        </label>
    };
    let export = move |_| {
        leptos::task::spawn_local(async move {
            if let Err(e) = api::download("/export.csv", "pebblelab-fin.csv").await {
                app.fail(&e);
            }
        });
    };
    view! {
        <div class="set__grid">
            <section class="d-card" aria-labelledby="pr-t">
                <header class="d-card__head"><h2 class="d-card__title" id="pr-t">"display"</h2></header>
                <div class="d-card__body">
                    <div class="set__row">
                        <label for="pr-cur">"currency"</label>
                        <span class="d-select" style="width:140px">
                            <select class="d-input" id="pr-cur" on:change=move |e| patch_me(app, serde_json::json!({ "currency": event_target_value(&e) }), move |r| if let Err(m) = r { app.error(m) })>
                                {[("inr", "inr, ₹"), ("usd", "usd, $"), ("eur", "eur, €")].into_iter().map(|(v, l)| view! { <option value=v selected=move || app.currency() == v>{l}</option> }).collect_view()}
                            </select>
                        </span>
                    </div>
                    <div class="set__row">
                        <span id="pr-th">"theme"</span>
                        <div class="seg" role="group" aria-labelledby="pr-th" style="width:140px">
                            <button type="button" class="seg__opt" aria-pressed=move || (theme.get() == Theme::Dark).to_string() aria-label="dark" title="dark" on:click=move |_| theme.set(Theme::Dark)><Ico d=MOON/></button>
                            <button type="button" class="seg__opt" aria-pressed=move || (theme.get() == Theme::Light).to_string() aria-label="light" title="light" on:click=move |_| theme.set(Theme::Light)><Ico d=SUN/></button>
                        </div>
                    </div>
                </div>
            </section>
            <section class="d-card" aria-labelledby="nt-t">
                <header class="d-card__head"><h2 class="d-card__title" id="nt-t">"notify me about"</h2></header>
                <div class="d-card__body">
                    {switch("card due dates", "notify_card", |u| u.notify_card)}
                    {switch("loan emis", "notify_emi", |u| u.notify_emi)}
                    {move || app.has_family().then(|| switch("transactions on joint accounts", "notify_joint", |u| u.notify_joint))}
                </div>
            </section>
            <section class="d-card" aria-labelledby="dx-t">
                <header class="d-card__head"><h2 class="d-card__title" id="dx-t">"your data"</h2></header>
                <div class="d-card__body">
                    <div class="set__row"><span>"accounts and transactions, as csv"</span><button type="button" class="d-btn d-btn--secondary" on:click=export><Ico d=DOWNLOAD/>"export"</button></div>
                </div>
            </section>
        </div>
    }
}

#[component]
fn Security() -> impl IntoView {
    let app = expect_context::<AppState>();
    let (cur, new) = (RwSignal::new(String::new()), RwSignal::new(String::new()));
    let err = RwSignal::new(None::<String>);
    let confirm = RwSignal::new(false);
    let (del_pw, del_err) = (RwSignal::new(String::new()), RwSignal::new(None::<String>));
    let change = move || {
        err.set(None);
        let body = serde_json::json!({"current": cur.get_untracked(), "new": new.get_untracked()});
        leptos::task::spawn_local(async move {
            match api::post::<serde_json::Value>("/me/password", &body).await {
                Ok(_) => {
                    // changing the password signs out every session, this one included
                    api::clear_token();
                    app.me.set(None);
                }
                Err(e) if e.status == 400 || e.status == 403 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let out_all = move |_| {
        leptos::task::spawn_local(async move {
            let _ = api::post::<serde_json::Value>("/auth/signout-all", &serde_json::json!({})).await;
            api::clear_token();
            app.me.set(None);
        });
    };
    let delete = move |_| {
        del_err.set(None);
        let pw = del_pw.get_untracked();
        leptos::task::spawn_local(async move {
            match api::send::<serde_json::Value>("DELETE", "/me", &serde_json::json!({ "password": pw })).await {
                Ok(_) => {
                    api::clear_token();
                    app.me.set(None);
                }
                Err(e) if e.status == 400 || e.status == 403 => del_err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    view! {
        <div class="set__grid">
            <form class="d-card" aria-labelledby="pw-t" on:submit=move |e| { e.prevent_default(); change() }>
                <header class="d-card__head"><h2 class="d-card__title" id="pw-t">"password"</h2></header>
                <div class="d-card__body" style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(260px,100%),1fr));gap:16px">
                    <div class="d-field">
                        <label class="d-label" for="pw-cur">"current password"</label>
                        <input class="d-input" id="pw-cur" type="password" autocomplete="current-password" prop:value=move || cur.get() on:input=move |e| cur.set(event_target_value(&e))/>
                    </div>
                    <div class="d-field">
                        <label class="d-label" for="pw-new">"new password"</label>
                        <input class="d-input" id="pw-new" type="password" autocomplete="new-password" aria-describedby="pw-new-h" prop:value=move || new.get() on:input=move |e| new.set(event_target_value(&e))/>
                        <span class="d-hint" id="pw-new-h">"12 characters or more. changing it signs you out everywhere."</span>
                    </div>
                </div>
                {move || err.get().map(|m| view! { <div class="d-card__body" style="padding-top:0"><span class="d-error" role="alert">{format!("error: {m}")}</span></div> })}
                <footer class="d-card__foot"><button type="submit" class="d-btn d-btn--secondary">"change password"</button></footer>
            </form>
            <section class="d-card" aria-labelledby="sec-t">
                <header class="d-card__head"><h2 class="d-card__title" id="sec-t">"sessions"</h2></header>
                <div class="d-card__body">
                    <div class="set__row"><span>"sign out everywhere, including this device"</span><button type="button" class="d-btn d-btn--secondary" on:click=out_all><Ico d=SIGN_OUT/>"sign out all"</button></div>
                </div>
            </section>
            <section class="d-card" aria-labelledby="dl-t">
                <header class="d-card__head"><h2 class="d-card__title" id="dl-t">"delete your account"</h2></header>
                <div class="d-card__body">
                    <div class="set__row"><span>"removes your sign-in, accounts and transactions. this cannot be undone."</span><button type="button" class="d-btn d-btn--danger" on:click=move |_| confirm.set(true)><Ico d=TRASH/>"delete"</button></div>
                </div>
            </section>
        </div>
        <Dialog open=confirm title="delete your account" dismiss=false footer=move || view! {
            <Button on:click=move |_| confirm.set(false)>"cancel"</Button>
            <Button variant=ButtonVariant::Danger on:click=delete>"delete my account"</Button>
        }>
            <div class="d-stack" style="gap:var(--space-12)">
                <p>"this removes your sign-in, the accounts only you own, and their transactions. joint accounts stay with the other owners. it cannot be undone."</p>
                <TextField label="your password, to confirm" value=del_pw input_type="password" autocomplete="current-password" error=del_err/>
            </div>
        </Dialog>
    }
}

#[component]
fn FamilyTab() -> impl IntoView {
    let app = expect_context::<AppState>();
    let name = RwSignal::new(String::new());
    let code = RwSignal::new(String::new());
    let (create_err, join_err) = (RwSignal::new(None::<String>), RwSignal::new(None::<String>));
    let confirm = RwSignal::new(false);
    let call = move |method: &'static str, path: &'static str, body: serde_json::Value, err: Option<RwSignal<Option<String>>>| {
        leptos::task::spawn_local(async move {
            match api::send::<serde_json::Value>(method, path, &body).await {
                Ok(_) => {
                    confirm.set(false);
                    app.reload();
                }
                Err(e) if e.status != 401 && e.status < 500 && e.status != 0 => match err {
                    Some(s) => s.set(Some(e.message)),
                    None => app.error(e.message),
                },
                Err(e) => app.fail(&e),
            }
        });
    };
    let family = Memo::new(move |_| app.me.get().and_then(|m| m.family));
    let me = move || app.my_id();
    view! {
        <div style="display:flex;flex-direction:column;gap:16px">
            {move || match family.get() {
                None => view! {
                    <div style="display:flex;flex-direction:column;gap:16px">
                        <p style="max-width:64ch">"family is optional. your accounts work the same without one. create a family to share accounts and hold joint ones, or join one with a code someone sent you."</p>
                        <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(min(300px,100%),1fr));gap:12px;align-items:start">
                            <form class="d-card" aria-labelledby="fc-t" on:submit=move |e| { e.prevent_default(); create_err.set(None); call("POST", "/family", serde_json::json!({"name": name.get_untracked()}), Some(create_err)) }>
                                <header class="d-card__head"><h2 class="d-card__title" id="fc-t">"create a family"</h2></header>
                                <div class="d-card__body">
                                    <div class="d-field">
                                        <label class="d-label" for="fc-name">"family name"</label>
                                        <input class="d-input" id="fc-name" type="text" aria-describedby="fc-name-h" prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e))/>
                                        <span class="d-hint" id="fc-name-h">"you become the owner. invite others after this."</span>
                                        {move || create_err.get().map(|m| view! { <span class="d-error" role="alert">{format!("error: {m}")}</span> })}
                                    </div>
                                </div>
                                <footer class="d-card__foot"><button type="submit" class="d-btn d-btn--primary">"create family"</button></footer>
                            </form>
                            <form class="d-card" aria-labelledby="fj-t" on:submit=move |e| { e.prevent_default(); join_err.set(None); call("POST", "/family/join", serde_json::json!({"code": code.get_untracked()}), Some(join_err)) }>
                                <header class="d-card__head"><h2 class="d-card__title" id="fj-t">"join a family"</h2></header>
                                <div class="d-card__body">
                                    <div class="d-field">
                                        <label class="d-label" for="fj-code">"invite code"</label>
                                        <input class="d-input" id="fj-code" type="text" autocomplete="off" aria-describedby="fj-code-h" prop:value=move || code.get() on:input=move |e| code.set(event_target_value(&e))/>
                                        <span class="d-hint" id="fj-code-h">"a one-time code from the family owner"</span>
                                        {move || join_err.get().map(|m| view! { <span class="d-error" role="alert">{format!("error: {m}")}</span> })}
                                    </div>
                                </div>
                                <footer class="d-card__foot"><button type="submit" class="d-btn d-btn--secondary">"join family"</button></footer>
                            </form>
                        </div>
                    </div>
                }.into_any(),
                Some(f) => {
                    let owner = f.owner_id == me();
                    let rows = f.members.iter().map(|m| {
                        let name = if m.id == me() { format!("{} (you)", m.name) } else { m.name.clone() };
                        (m.initials.clone(), name, m.email.clone(), if m.id == f.owner_id { "owner" } else { "member" })
                    }).collect::<Vec<_>>();
                    let code = f.invite_code.clone();
                    let has_code = code.is_some();
                    view! {
                        <div style="display:flex;flex-direction:column;gap:16px">
                            <div class="d-table-wrap" tabindex="0" role="region" aria-label="family members">
                                <table class="d-table d-table--lg">
                                    <thead><tr><th scope="col">"person"</th><th scope="col">"email"</th><th scope="col">"role"</th><th scope="col">"status"</th></tr></thead>
                                    <tbody>
                                        {rows.into_iter().map(|(initials, name, email, role)| view! {
                                            <tr>
                                                <td><span class="d-row" style="gap:8px;flex-wrap:nowrap"><span class="d-avatar d-avatar--sm" aria-hidden="true">{initials}</span>{name}</span></td>
                                                <td>{email}</td><td>{role}</td>
                                                <td><span class="d-status" data-state="on" data-severity="success">"active"</span></td>
                                            </tr>
                                        }).collect_view()}
                                    </tbody>
                                </table>
                            </div>
                            {if owner {
                                view! {
                                    <section class="d-card" aria-labelledby="inv-t">
                                        <header class="d-card__head"><h2 class="d-card__title" id="inv-t">"invite a member"</h2><span class="d-card__meta">"one-time code"</span></header>
                                        <div class="d-card__body" style="display:flex;flex-direction:column;gap:12px;align-items:flex-start">
                                            <p style="max-width:64ch">"generate a code and send it yourself. they enter it under settings, family, join a family. each code works once."</p>
                                            {code.map(|c| { let c2 = c.clone(); view! {
                                                <div class="d-field" style="width:100%;max-width:320px">
                                                    <label class="d-label" for="inv-code">"invite code"</label>
                                                    <div class="d-row" style="flex-wrap:nowrap;gap:4px">
                                                        <input class="d-input d-input--lg" id="inv-code" type="text" readonly=true value=c aria-describedby="inv-code-h" style="flex:1;min-width:0;font-size:var(--font-size-lg);color:var(--text-strong)"/>
                                                        <button type="button" class="d-btn d-btn--ghost d-btn--icon" aria-label="copy invite code" title="copy" on:click=move |_| copy(app, c2.clone())><Ico d=COPY/></button>
                                                    </div>
                                                    <span class="d-hint" id="inv-code-h">"not used yet. a new code replaces this one."</span>
                                                </div>
                                            } })}
                                            <button type="button" class="d-btn d-btn--secondary" on:click=move |_| call("POST", "/family/invite", serde_json::json!({}), None)>{if has_code { "generate a new code" } else { "generate invite code" }}</button>
                                        </div>
                                    </section>
                                    <section class="d-card" aria-labelledby="fx-t">
                                        <header class="d-card__head"><h2 class="d-card__title" id="fx-t">"delete family"</h2></header>
                                        <div class="d-card__body" style="display:flex;flex-direction:column;gap:12px;align-items:flex-start">
                                            <p style="max-width:64ch">"everyone goes back to a personal account and keeps what they own. shared access ends."</p>
                                            <button type="button" class="d-btn d-btn--danger" on:click=move |_| confirm.set(true)>"delete family"</button>
                                        </div>
                                    </section>
                                }.into_any()
                            } else {
                                view! {
                                    <section class="d-card" aria-labelledby="fx-t">
                                        <header class="d-card__head"><h2 class="d-card__title" id="fx-t">"leave family"</h2></header>
                                        <div class="d-card__body" style="display:flex;flex-direction:column;gap:12px;align-items:flex-start">
                                            <p style="max-width:64ch">"your accounts become private again and you drop off joint ones. nothing is deleted. only the owner can invite people."</p>
                                            <button type="button" class="d-btn d-btn--danger" on:click=move |_| confirm.set(true)>"leave family"</button>
                                        </div>
                                    </section>
                                }.into_any()
                            }}
                        </div>
                    }.into_any()
                }
            }}
        </div>
        {move || {
            let owner = family.get().is_some_and(|f| f.owner_id == me());
            let (title, text, path, method) = if owner {
                ("delete family", "everyone goes back to a personal account and keeps what they own. shared access ends.", "/family", "DELETE")
            } else {
                ("leave family", "your accounts become private again and you drop off joint ones.", "/family/leave", "POST")
            };
            view! {
                <Dialog open=confirm title=title footer=move || view! {
                    <Button on:click=move |_| confirm.set(false)>"cancel"</Button>
                    <Button variant=ButtonVariant::Danger on:click=move |_| call(method, path, serde_json::json!({}), None)>{title}</Button>
                }>
                    <p>{text}</p>
                </Dialog>
            }
        }}
    }
}

/// A read-only box with a copy button: for commands and config the person pastes somewhere else.
#[component]
fn CopyBox(label: &'static str, #[prop(into)] value: Signal<String>, #[prop(optional)] hint: &'static str) -> impl IntoView {
    let app = expect_context::<AppState>();
    view! {
        <div class="d-field">
            <label class="d-label">{label}</label>
            <div class="d-row" style="flex-wrap:nowrap;gap:4px;align-items:flex-start">
                // rows is the fallback for browsers without field-sizing, which sizes the box to what it holds
                <textarea class="d-input" readonly=true rows=move || value.with(|v| v.lines().count().max(1)) aria-label=label prop:value=move || value.get()
                    style="flex:1;min-width:0;height:auto;min-height:0;field-sizing:content;resize:none;overflow:hidden;font-family:var(--font-mono, monospace);font-size:12px;line-height:1.5;padding:8px 10px"></textarea>
                <button type="button" class="d-btn d-btn--ghost d-btn--icon" aria-label=format!("copy {label}") title="copy" on:click=move |_| copy(app, value.get_untracked())><Ico d=COPY/></button>
            </div>
            {(!hint.is_empty()).then(|| view! { <span class="d-hint">{hint}</span> })}
        </div>
    }
}

const PERMS: [(&str, &str); 4] = [
    ("read accounts and balances", "read"),
    ("read transactions", "transactions"),
    ("add transactions", "add"),
    ("edit or delete transactions", "edit"),
];

/// Tokens for Claude and other clients. Each one has its own name and its own permissions, so one can be
/// changed or cut off without touching the others.
#[component]
fn Connectors() -> impl IntoView {
    let app = expect_context::<AppState>();
    let list = RwSignal::new(Vec::<Connector>::new());
    let loaded = RwSignal::new(false);
    // the token just made, with its name: the only time it is shown in full
    let fresh = RwSignal::new(None::<(String, String)>);
    let creating = RwSignal::new(false);
    let (new_name, busy) = (RwSignal::new(String::new()), RwSignal::new(false));
    let new_scopes = RwSignal::new(vec!["read".to_string(), "transactions".to_string()]);
    let revoke = RwSignal::new(None::<Connector>);
    let confirm = RwSignal::new(false);
    leptos::task::spawn_local(async move {
        if let Ok(l) = api::get::<Vec<Connector>>("/connectors").await {
            list.set(l);
        }
        loaded.set(true);
    });
    let url = format!("{}/mcp", web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default());
    let url2 = url.clone();
    let (url3, url4) = (url.clone(), url.clone());
    let tok = move || fresh.with(|f| f.as_ref().map(|(_, t)| t.clone())).unwrap_or_else(|| "YOUR_TOKEN".to_string());
    let create = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let name = new_name.get_untracked();
        if name.trim().is_empty() {
            return app.error("name the token after the client that will use it, e.g. claude code");
        }
        busy.set(true);
        leptos::task::spawn_local(async move {
            match api::post::<CreatedConnector>("/connectors", &serde_json::json!({ "name": name, "scopes": new_scopes.get_untracked() })).await {
                Ok(c) => {
                    fresh.set(Some((c.connector.name.clone(), c.token)));
                    list.update(|l| l.push(c.connector));
                    creating.set(false);
                    new_name.set(String::new());
                }
                Err(e) => app.fail(&e),
            }
            busy.set(false);
        });
    };
    let set_scope = move |id: i64, scope: &'static str, on: bool| {
        let Some(c) = list.with_untracked(|l| l.iter().find(|c| c.id == id).cloned()) else { return };
        let mut scopes: Vec<String> = c.scopes.into_iter().filter(|s| s != scope).collect();
        if on {
            scopes.push(scope.into());
        }
        leptos::task::spawn_local(async move {
            match api::patch::<Connector>(&format!("/connectors/{id}"), &serde_json::json!({ "scopes": scopes })).await {
                Ok(c) => list.update(|l| l.iter_mut().filter(|x| x.id == id).for_each(|x| *x = c.clone())),
                Err(e) => app.fail(&e),
            }
        });
    };
    let do_revoke = move |_| {
        let Some(c) = revoke.get_untracked() else { return };
        confirm.set(false);
        leptos::task::spawn_local(async move {
            match api::delete(&format!("/connectors/{}", c.id)).await {
                Ok(_) => {
                    list.update(|l| l.retain(|x| x.id != c.id));
                    app.ok("token revoked");
                }
                Err(e) => app.fail(&e),
            }
        });
    };
    let day = |s: &str| crate::fmt::day(s.get(..10).unwrap_or(""));
    view! {
        <div style="display:flex;flex-direction:column;gap:16px;max-width:640px">
            <section class="d-card" aria-labelledby="cn-e">
                <header class="d-card__head"><h2 class="d-card__title" id="cn-e">"server address (mcp)"</h2></header>
                <div class="d-card__body" style="display:flex;flex-direction:column;gap:8px">
                    <div class="d-row" style="flex-wrap:nowrap;gap:4px">
                        <input class="d-input" id="cn-url" type="text" readonly=true aria-label="server address" value=url style="flex:1;min-width:0"/>
                        <button type="button" class="d-btn d-btn--ghost d-btn--icon" aria-label="copy server address" title="copy" on:click=move |_| copy(app, url2.clone())><Ico d=COPY/></button>
                    </div>
                    <span class="d-hint">"clients send a token as a bearer token. make one per client below."</span>
                </div>
            </section>
            <section class="d-card" aria-labelledby="cn-c">
                <header class="d-card__head"><h2 class="d-card__title" id="cn-c">"connect a client"</h2></header>
                <div class="d-card__body" style="display:flex;flex-direction:column;gap:16px">
                    <span class="d-hint">{move || if fresh.with(|f| f.is_some()) { "these include the token you just made." } else { "replace YOUR_TOKEN with a token from below. a token is only shown in full when it is made." }}</span>
                    <CopyBox label="claude code (terminal)" value=Signal::derive(move || format!("claude mcp add --transport http pebblelab {} \\\n  --header \"Authorization: Bearer {}\"", url3, tok()))/>
                    <CopyBox label="config file (.mcp.json and other clients)" value=Signal::derive(move || format!("{{\n  \"mcpServers\": {{\n    \"pebblelab\": {{\n      \"type\": \"http\",\n      \"url\": \"{}\",\n      \"headers\": {{ \"Authorization\": \"Bearer {}\" }}\n    }}\n  }}\n}}", url4, tok()))/>
                    <CopyBox label="claude.ai and the mobile app: header value" value=Signal::derive(move || format!("Bearer {}", tok())) hint="claude.ai, then customize, connectors, add custom connector. paste the server address, add a header named Authorization with this value. it then shows in the mobile app too. the server must be reachable from the internet over https."/>
                </div>
            </section>
            {move || fresh.get().map(|(name, token)| {
                let t2 = token.clone();
                view! {
                    <section class="d-card" aria-labelledby="cn-new" role="status">
                        <header class="d-card__head"><h2 class="d-card__title" id="cn-new">{format!("token for {name}")}</h2></header>
                        <div class="d-card__body" style="display:flex;flex-direction:column;gap:8px">
                            <div class="d-row" style="flex-wrap:nowrap;gap:4px">
                                <input class="d-input" type="text" readonly=true aria-label="new token" value=token style="flex:1;min-width:0"/>
                                <button type="button" class="d-btn d-btn--ghost d-btn--icon" aria-label="copy token" title="copy" on:click=move |_| copy(app, t2.clone())><Ico d=COPY/></button>
                            </div>
                            <span class="d-hint">"copy it now: it is shown in full only this once."</span>
                        </div>
                        <footer class="d-card__foot"><button type="button" class="d-btn" on:click=move |_| fresh.set(None)>"done"</button></footer>
                    </section>
                }
            })}
            <div class="d-row" style="justify-content:space-between">
                <h2 class="d-card__title">"tokens"</h2>
                <button type="button" class="d-btn d-btn--primary" disabled=move || creating.get() on:click=move |_| creating.set(true)><Ico d=PLUS/>"new token"</button>
            </div>
            {move || creating.get().then(|| view! {
                <form class="d-card" on:submit=create aria-label="new token">
                    <div class="d-card__body" style="display:flex;flex-direction:column;gap:12px">
                        <div class="d-field">
                            <label class="d-label" for="cn-name">"name"</label>
                            <input class="d-input" id="cn-name" type="text" placeholder="claude code" autocomplete="off" prop:value=move || new_name.get() on:input=move |e| new_name.set(event_target_value(&e))/>
                        </div>
                        <div>
                            {PERMS.map(|(label, scope)| view! {
                                <label class="set__row"><span>{label}</span>
                                    <input type="checkbox" role="switch" class="d-switch" prop:checked=move || new_scopes.with(|l| l.iter().any(|s| s == scope))
                                        on:change=move |e| {
                                            let on = event_target_checked(&e);
                                            new_scopes.update(|l| { l.retain(|s| s != scope); if on { l.push(scope.into()); } });
                                        }/>
                                </label>
                            })}
                        </div>
                    </div>
                    <footer class="d-card__foot">
                        <button type="button" class="d-btn" on:click=move |_| creating.set(false)>"cancel"</button>
                        <button type="submit" class="d-btn d-btn--primary" disabled=move || busy.get()><Ico d=CHECK/>"make token"</button>
                    </footer>
                </form>
            })}
            {move || (loaded.get() && list.with(|l| l.is_empty()) && !creating.get()).then(|| view! {
                <p class="d-hint" style="margin:0">"no tokens yet. make one for each client you connect, so you can change or cut one off without touching the others."</p>
            })}
            <For each=move || list.get() key=|c| c.id children=move |c| {
                let id = c.id;
                let meta = format!("created {}{}", day(&c.created_at), c.last_used_at.as_deref().map(|u| format!(", last used {}", day(u))).unwrap_or_else(|| ", never used".into()));
                let c2 = c.clone();
                view! {
                    <section class="d-card" aria-label=format!("token {}", c.name)>
                        <header class="d-card__head" style="display:flex;align-items:center;justify-content:space-between;gap:8px;height:auto;padding-top:12px;padding-bottom:12px">
                            <div style="display:flex;flex-direction:column;gap:2px;min-width:0">
                                <h3 class="d-card__title">{c.name.clone()}<span class="d-badge" style="margin-left:8px">{format!("…{}", c.tail)}</span></h3>
                                <span class="d-hint">{meta}</span>
                            </div>
                            <button type="button" class="d-btn d-btn--ghost d-btn--icon" aria-label=format!("revoke token {}", c.name) title="revoke"
                                on:click=move |_| { revoke.set(Some(c2.clone())); confirm.set(true); }><Ico d=TRASH/></button>
                        </header>
                        <div class="d-card__body">
                            {PERMS.map(|(label, scope)| view! {
                                <label class="set__row"><span>{label}</span>
                                    <input type="checkbox" role="switch" class="d-switch"
                                        prop:checked=move || list.with(|l| l.iter().any(|x| x.id == id && x.scopes.iter().any(|s| s == scope)))
                                        on:change=move |e| set_scope(id, scope, event_target_checked(&e))/>
                                </label>
                            })}
                        </div>
                    </section>
                }
            }/>
        </div>
        <Dialog open=confirm title="revoke token" footer=move || view! {
            <Button on:click=move |_| confirm.set(false)>"cancel"</Button>
            <Button variant=ButtonVariant::Danger on:click=do_revoke>"revoke"</Button>
        }>
            <p>{move || revoke.get().map(|c| format!("{} stops working right away. clients using it will need a new token.", c.name)).unwrap_or_default()}</p>
        </Dialog>
    }
}

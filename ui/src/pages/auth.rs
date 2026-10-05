use dots_design::prelude::*;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use tracer_api::*;

use crate::api;

/// `/`: to the app when signed in, else to sign in.
#[component]
pub fn Home() -> impl IntoView {
    let nav = use_navigate();
    Effect::new(move |_| {
        nav(if api::load_token().is_some() { "/transactions" } else { "/signin" }, Default::default());
    });
}

#[component]
fn AuthPage(title: &'static str, sub: &'static str, other: AnyView, children: Children) -> impl IntoView {
    view! {
        <SkipLink/>
        <div style="min-height:100dvh;display:flex;flex-direction:column;background:var(--bg-base)">
            <AppBar>
                <A href="/signin" attr:class="d-appbar__title" attr:style="text-decoration:none">"tracer/fin"</A>
                <AppBarSpacer/>
                {other}
            </AppBar>
            <main id="main" tabindex="-1" style="flex:1;display:flex;align-items:center;justify-content:center;padding:48px 16px">
                <div class="d-card" style="width:100%;max-width:380px">
                    <div class="d-card__body" style="padding:24px;display:flex;flex-direction:column;gap:16px">
                        <div style="display:flex;flex-direction:column;gap:4px">
                            <h1 class="heading-xl">{title}</h1>
                            <p style="color:var(--text-secondary)">{sub}</p>
                        </div>
                        {children()}
                    </div>
                </div>
            </main>
        </div>
    }
}

fn other(prompt: &'static str, href: &'static str, label: &'static str) -> AnyView {
    view! {
        <span style="color:var(--text-secondary)">{prompt}</span>
        <ButtonLink href=href variant=ButtonVariant::Secondary size=Size::Sm>{label}</ButtonLink>
    }
    .into_any()
}

#[component]
pub fn SignIn() -> impl IntoView {
    let (email, pass, keep) = (RwSignal::new(String::new()), RwSignal::new(String::new()), RwSignal::new(true));
    let (busy, err) = (RwSignal::new(false), RwSignal::new(None::<String>));
    let nav = use_navigate();
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        busy.set(true);
        err.set(None);
        let nav = nav.clone();
        leptos::task::spawn_local(async move {
            match api::post::<Session>("/auth/signin", &serde_json::json!({"email": email.get_untracked(), "password": pass.get_untracked()})).await {
                Ok(s) => {
                    api::save_token(&s.token, keep.get_untracked());
                    nav("/transactions", Default::default());
                }
                Err(e) => {
                    err.set(Some(if e.unauthorized() { "email or password is wrong".into() } else { e.message }));
                    busy.set(false);
                }
            }
        });
    };
    view! {
        <AuthPage title="sign in" sub="to your account." other=other("new here?", "/signup", "create an account")>
            <form on:submit=submit style="display:flex;flex-direction:column;gap:16px">
                <TextField label="email" value=email input_type="email" size=Size::Lg autocomplete="email" error=err/>
                <TextField label="password" value=pass input_type="password" size=Size::Lg autocomplete="current-password"/>
                <div class="d-row" style="justify-content:space-between">
                    <Checkbox checked=keep>"keep me signed in"</Checkbox>
                    <A href="/reset" attr:class="d-link">"forgot password"</A>
                </div>
                <Button variant=ButtonVariant::Primary size=Size::Lg submit=true busy=busy attr:style="width:100%">"sign in"</Button>
            </form>
        </AuthPage>
    }
}

#[component]
pub fn SignUp() -> impl IntoView {
    let (name, email, pass) = (RwSignal::new(String::new()), RwSignal::new(String::new()), RwSignal::new(String::new()));
    let (busy, err) = (RwSignal::new(false), RwSignal::new(None::<String>));
    let nav = use_navigate();
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        busy.set(true);
        err.set(None);
        let nav = nav.clone();
        leptos::task::spawn_local(async move {
            match api::post::<Session>("/auth/signup", &serde_json::json!({"name": name.get_untracked(), "email": email.get_untracked(), "password": pass.get_untracked()})).await {
                Ok(s) => {
                    api::save_token(&s.token, true);
                    nav("/transactions", Default::default());
                }
                Err(e) => {
                    err.set(Some(e.message));
                    busy.set(false);
                }
            }
        });
    };
    view! {
        <AuthPage title="create an account" sub="your money, in one place." other=other("have an account?", "/signin", "sign in")>
            <form on:submit=submit style="display:flex;flex-direction:column;gap:16px">
                <TextField label="name" value=name size=Size::Lg autocomplete="name"/>
                <TextField label="email" value=email input_type="email" size=Size::Lg autocomplete="email"/>
                <TextField label="password" value=pass input_type="password" size=Size::Lg autocomplete="new-password" hint="at least 8 characters" error=err/>
                <Button variant=ButtonVariant::Primary size=Size::Lg submit=true busy=busy attr:style="width:100%">"create account"</Button>
            </form>
        </AuthPage>
    }
}

#[component]
pub fn Reset() -> impl IntoView {
    let email = RwSignal::new(String::new());
    let (busy, sent) = (RwSignal::new(false), RwSignal::new(false));
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        busy.set(true);
        leptos::task::spawn_local(async move {
            let _ = api::post::<serde_json::Value>("/auth/reset", &serde_json::json!({"email": email.get_untracked()})).await;
            busy.set(false);
            sent.set(true);
        });
    };
    view! {
        <AuthPage title="reset password" sub="we do not send email yet." other=other("remembered it?", "/signin", "sign in")>
            {move || if sent.get() {
                view! {
                    <p>"if that address has an account, an administrator can reset it with:"</p>
                    <code class="d-code">"tracer user passwd <email>"</code>
                    <A href="/signin" attr:class="d-link">"back to sign in"</A>
                }.into_any()
            } else {
                view! {
                    <form on:submit=submit style="display:flex;flex-direction:column;gap:16px">
                        <TextField label="email" value=email input_type="email" size=Size::Lg autocomplete="email"/>
                        <Button variant=ButtonVariant::Primary size=Size::Lg submit=true busy=busy attr:style="width:100%">"request reset"</Button>
                    </form>
                }.into_any()
            }}
        </AuthPage>
    }
}

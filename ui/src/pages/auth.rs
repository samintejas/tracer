use dots_ui::prelude::*;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use tracer_api::*;

use crate::api;

#[component]
fn AuthPage(title: &'static str, sub: &'static str, other: AnyView, #[prop(default = 380)] width: u32, children: Children) -> impl IntoView {
    view! {
        <SkipLink/>
        <div style="min-height:100dvh;display:flex;flex-direction:column;background:var(--bg-base)">
            <AppBar>
                <A href="/" attr:class="d-appbar__title" attr:style="text-decoration:none">"tracer/fin"</A>
                <AppBarSpacer/>
                {other}
            </AppBar>
            <main id="main" tabindex="-1" style="flex:1;display:flex;align-items:center;justify-content:center;padding:48px 16px">
                <div class="d-card" style=format!("width:100%;max-width:{width}px")>
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

/// "or continue with google / github", for the providers this server has set up. Nothing shows when none are.
#[component]
fn Providers(verb: &'static str) -> impl IntoView {
    let list = LocalResource::new(api::providers);
    view! {
        {move || list.get().filter(|l| !l.is_empty()).map(|l| view! {
            <div style="display:flex;flex-direction:column;gap:8px">
                <div class="d-row" style="gap:12px;color:var(--text-secondary)" aria-hidden="true">
                    <span style="flex:1;border-top:1px solid var(--border-default)"></span>"or"<span style="flex:1;border-top:1px solid var(--border-default)"></span>
                </div>
                {l.into_iter().map(|p| view! {
                    // a full page load: the provider is another site, so the router must leave it alone
                    <a class="d-btn d-btn--secondary d-btn--lg" rel="external" href=format!("/api/auth/{p}/start") style="width:100%;justify-content:center">
                        {format!("{verb} with {p}")}
                    </a>
                }).collect_view()}
            </div>
        })}
    }
}

/// Where a provider sends the person after they sign in: swap the one-time code in the address for a session.
#[component]
pub fn AuthCallback() -> impl IntoView {
    let err = RwSignal::new(None::<String>);
    let nav = use_navigate();
    leptos::task::spawn_local(async move {
        let Some(code) = api::hash_params().remove("code") else {
            return err.set(Some("there is no sign-in to finish: start again".into()));
        };
        match api::post::<Session>("/auth/redeem", &serde_json::json!({ "code": code })).await {
            Ok(s) => {
                api::save_token(&s.token, true);
                nav("/transactions", Default::default());
            }
            Err(e) => err.set(Some(if e.unauthorized() { "this sign-in expired or was already used: start again".into() } else { e.message })),
        }
    });
    view! {
        <AuthPage title="signing you in" sub="one moment." other=other("", "/signin", "sign in")>
            {move || err.get().map(|m| view! {
                <div role="alert" style="display:flex;flex-direction:column;gap:12px">
                    <span class="d-error">{format!("error: {m}")}</span>
                    <A href="/signin" attr:class="d-link" attr:style="align-self:flex-start">"back to sign in"</A>
                </div>
            })}
        </AuthPage>
    }
}

#[component]
pub fn SignIn() -> impl IntoView {
    let (email, pass, keep) = (RwSignal::new(String::new()), RwSignal::new(String::new()), RwSignal::new(true));
    // a provider sign-in that failed lands here with the reason after the `#`
    let (busy, err) = (RwSignal::new(false), RwSignal::new(api::hash_params().remove("error")));
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
            <Providers verb="sign in"/>
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
        <AuthPage title="create an account" sub="for you alone. a family is optional and can be added later in settings." width=420 other=other("have an account?", "/signin", "sign in")>
            <form on:submit=submit style="display:flex;flex-direction:column;gap:16px">
                <TextField label="name" value=name size=Size::Lg autocomplete="name"/>
                <TextField label="email" value=email input_type="email" size=Size::Lg autocomplete="email"/>
                <TextField label="password" value=pass input_type="password" size=Size::Lg autocomplete="new-password" hint="12 characters or more" error=err/>
                <Button variant=ButtonVariant::Primary size=Size::Lg submit=true busy=busy attr:style="width:100%">"create account"</Button>
            </form>
            <Providers verb="sign up"/>
        </AuthPage>
    }
}

#[component]
pub fn Reset() -> impl IntoView {
    let only_sign_in = view! { <ButtonLink href="/signin" variant=ButtonVariant::Secondary size=Size::Sm>"sign in"</ButtonLink> }.into_any();
    view! {
        <AuthPage title="reset password" sub="this server cannot send email yet, so a password is reset by whoever runs it." other=only_sign_in>
            <div role="status" style="display:flex;flex-direction:column;gap:16px">
                <p style="margin:0;max-width:64ch">"ask the person who runs this server to reset it for you. they run " <code>"tracer user passwd your@email"</code> " and give you a new one. you can change it again under profile."</p>
                <A href="/signin" attr:class="d-link" attr:style="align-self:flex-start">"back to sign in"</A>
            </div>
        </AuthPage>
    }
}

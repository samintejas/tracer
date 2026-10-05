mod api;
mod fmt;
mod pages;
mod state;

use dots_design::prelude::*;
use leptos::prelude::*;
use leptos_router::components::{Redirect, Route, Router, Routes};
use leptos_router::path;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    view! {
        <DotsStyles font_base="/fonts/"/>
        <DotsProvider persist_theme="tracer:theme">
            <Router>
                <Routes fallback=|| view! { <Redirect path="/"/> }>
                    <Route path=path!("/") view=pages::Home/>
                    <Route path=path!("/signin") view=pages::SignIn/>
                    <Route path=path!("/signup") view=pages::SignUp/>
                    <Route path=path!("/reset") view=pages::Reset/>
                    <Route path=path!("/transactions") view=|| view! { <pages::AppShell page="transactions"><pages::Transactions/></pages::AppShell> }/>
                    <Route path=path!("/insights") view=|| view! { <pages::AppShell page="insights"><pages::Insights/></pages::AppShell> }/>
                    <Route path=path!("/accounts") view=|| view! { <pages::AppShell page="accounts"><pages::Accounts/></pages::AppShell> }/>
                    <Route path=path!("/profile") view=|| view! { <pages::AppShell page="profile"><pages::Profile/></pages::AppShell> }/>
                </Routes>
            </Router>
        </DotsProvider>
    }
}

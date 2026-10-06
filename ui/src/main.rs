mod api;
mod fmt;
mod icons;
mod pages;
mod state;

use dots_ui::prelude::*;
use leptos::prelude::*;
use leptos_router::components::{ParentRoute, Redirect, Route, Router, Routes};
use leptos_router::path;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    view! {
        <DotsStyles font_base="/fonts/"/>
        <style>{include_str!("app.css")}</style>
        <DotsProvider persist_theme="pebblelab:theme">
            <Router>
                <Routes fallback=|| view! { <Redirect path="/"/> }>
                    <Route path=path!("/") view=pages::Landing/>
                    <Route path=path!("/signin") view=pages::SignIn/>
                    <Route path=path!("/signup") view=pages::SignUp/>
                    <Route path=path!("/reset") view=pages::Reset/>
                    <Route path=path!("/auth/callback") view=pages::AuthCallback/>
                    <ParentRoute path=path!("") view=pages::AppShell>
                        <Route path=path!("transactions") view=pages::Transactions/>
                        <Route path=path!("insights") view=pages::Insights/>
                        <Route path=path!("subscriptions") view=pages::Subscriptions/>
                        <Route path=path!("assets") view=pages::Assets/>
                        <Route path=path!("accounts") view=pages::Accounts/>
                        <Route path=path!("accounts/new") view=pages::NewAccount/>
                        <Route path=path!("accounts/:id") view=pages::AccountPage/>
                        <Route path=path!("settings") view=pages::Settings/>
                        <Route path=path!("settings/:tab") view=pages::Settings/>
                    </ParentRoute>
                </Routes>
            </Router>
        </DotsProvider>
    }
}

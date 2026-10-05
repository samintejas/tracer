use dots_design::prelude::*;
use leptos::prelude::*;
use tracer_api::*;

use crate::api;
use crate::state::AppState;

#[component]
pub fn Profile() -> impl IntoView {
    let tab = RwSignal::new("settings".to_string());
    view! {
        <div style="display:flex;flex-direction:column;gap:16px;max-width:720px">
            <h1 class="heading-xl">"profile"</h1>
            <Tabs value=tab>
                <TabList label="profile sections">
                    <Tab value="settings">"settings"</Tab>
                    <Tab value="family">"family"</Tab>
                    <Tab value="connectors">"connectors"</Tab>
                </TabList>
                <TabPanel value="settings"><Settings/></TabPanel>
                <TabPanel value="family"><FamilyTab/></TabPanel>
                <TabPanel value="connectors"><Connectors/></TabPanel>
            </Tabs>
        </div>
    }
}

#[component]
fn Settings() -> impl IntoView {
    let app = expect_context::<AppState>();
    let me = move || app.me.get().map(|m| m.user);
    let name = RwSignal::new(me().map(|u| u.name).unwrap_or_default());
    let initials = RwSignal::new(me().map(|u| u.initials).unwrap_or_default());
    let email = RwSignal::new(me().map(|u| u.email).unwrap_or_default());
    let phone = RwSignal::new(me().map(|u| u.phone).unwrap_or_default());
    let cur = RwSignal::new(me().map(|u| u.currency).unwrap_or_else(|| "inr".into()));
    // the profile may load after this tab opens
    Effect::new(move |_| {
        if let Some(u) = me() {
            if name.get_untracked().is_empty() {
                name.set(u.name);
                initials.set(u.initials);
                email.set(u.email);
                phone.set(u.phone);
                cur.set(u.currency);
            }
        }
    });
    let (cur_pw, new_pw) = (RwSignal::new(String::new()), RwSignal::new(String::new()));
    let (err, pw_err) = (RwSignal::new(None::<String>), RwSignal::new(None::<String>));
    let save = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        err.set(None);
        let body = serde_json::json!({"name": name.get_untracked(), "initials": initials.get_untracked(), "email": email.get_untracked(), "phone": phone.get_untracked(), "currency": cur.get_untracked()});
        leptos::task::spawn_local(async move {
            match api::patch::<User>("/me", &body).await {
                Ok(_) => {
                    app.ok("saved");
                    app.reload();
                }
                Err(e) if e.status == 400 || e.status == 409 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let change = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        pw_err.set(None);
        let body = serde_json::json!({"current": cur_pw.get_untracked(), "new": new_pw.get_untracked()});
        leptos::task::spawn_local(async move {
            match api::post::<serde_json::Value>("/me/password", &body).await {
                Ok(_) => {
                    cur_pw.set(String::new());
                    new_pw.set(String::new());
                    app.ok("password changed");
                }
                Err(e) if e.status == 400 || e.status == 403 => pw_err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    view! {
        <div class="d-stack" style="gap:var(--space-16);padding-top:var(--space-16)">
            <Card>
                <CardHead title="you"/>
                <CardBody>
                    <form class="d-stack" style="gap:var(--space-12)" on:submit=save>
                        <TextField label="name" value=name/>
                        <TextField label="initials" value=initials hint="two or three letters on your avatar"/>
                        <TextField label="email" value=email input_type="email"/>
                        <TextField label="phone" value=phone optional=true/>
                        <Select label="currency" value=cur options=vec![SelectOption::new("inr", "₹ rupee, 1,00,000"), SelectOption::new("usd", "$ dollar, 100,000"), SelectOption::new("eur", "€ euro, 100,000")] hint="how amounts are written. your figures do not change."/>
                        {move || err.get().map(|m| view! { <span class="d-error" role="alert"><Icon name="octagon-alert" small=true/>{format!("error: {m}")}</span> })}
                        <div><Button variant=ButtonVariant::Primary submit=true>"save"</Button></div>
                    </form>
                </CardBody>
            </Card>
            <Card>
                <CardHead title="password"/>
                <CardBody>
                    <form class="d-stack" style="gap:var(--space-12)" on:submit=change>
                        <TextField label="current password" value=cur_pw input_type="password" autocomplete="current-password"/>
                        <TextField label="new password" value=new_pw input_type="password" autocomplete="new-password" hint="at least 8 characters" error=pw_err/>
                        <div><Button submit=true>"change password"</Button></div>
                    </form>
                </CardBody>
            </Card>
        </div>
    }
}

#[component]
fn FamilyTab() -> impl IntoView {
    let app = expect_context::<AppState>();
    let name = RwSignal::new(String::new());
    let code = RwSignal::new(String::new());
    let err = RwSignal::new(None::<String>);
    let confirm = RwSignal::new(false);
    let create = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let body = serde_json::json!({"name": name.get_untracked()});
        leptos::task::spawn_local(async move {
            match api::post::<Family>("/family", &body).await {
                Ok(_) => { app.ok("family created"); app.reload(); }
                Err(e) if e.status < 500 && e.status != 401 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let join = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let body = serde_json::json!({"code": code.get_untracked()});
        leptos::task::spawn_local(async move {
            match api::post::<Family>("/family/join", &body).await {
                Ok(_) => { app.ok("joined"); app.reload(); }
                Err(e) if e.status < 500 && e.status != 401 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let leave = move |_| {
        leptos::task::spawn_local(async move {
            match api::delete("/family").await {
                Ok(_) => { confirm.set(false); app.ok("left the family"); app.reload(); }
                Err(e) => app.fail(&e),
            }
        });
    };
    let copy = move |text: String| {
        if let Some(w) = web_sys::window() {
            let _ = w.navigator().clipboard().write_text(&text);
            app.ok("copied");
        }
    };
    view! {
        <div class="d-stack" style="gap:var(--space-16);padding-top:var(--space-16)">
            {move || match app.me.get().and_then(|m| m.family) {
                None => view! {
                    <Card>
                        <CardHead title="create a family"/>
                        <CardBody>
                            <form class="d-stack" style="gap:var(--space-12)" on:submit=create>
                                <TextField label="family name" value=name placeholder="rao family" hint="share accounts, see each other's shared money, and add to joint accounts together."/>
                                {move || err.get().map(|m| view! { <span class="d-error" role="alert"><Icon name="octagon-alert" small=true/>{format!("error: {m}")}</span> })}
                                <div><Button variant=ButtonVariant::Primary submit=true>"create family"</Button></div>
                            </form>
                        </CardBody>
                    </Card>
                    <Card>
                        <CardHead title="join a family"/>
                        <CardBody>
                            <form class="d-stack" style="gap:var(--space-12)" on:submit=join>
                                <TextField label="invite code" value=code placeholder="K7QF-2M9X"/>
                                <div><Button submit=true>"join family"</Button></div>
                            </form>
                        </CardBody>
                    </Card>
                }.into_any(),
                Some(f) => {
                    let invite = f.invite_code.clone();
                    let members_view = f.members.iter().map(|m| { let (n, i) = (m.name.clone(), m.initials.clone()); view! { <Identity name=n><Avatar initials=i decorative=true size=AvatarSize::Lg/></Identity> } }).collect_view();
                    view! {
                        <Card>
                            <CardHead title=f.name.clone() meta=format!("{} members", f.members.len())/>
                            <CardBody>
                                <div class="d-stack" style="gap:var(--space-12)">
                                    {members_view}
                                </div>
                            </CardBody>
                        </Card>
                        <Card>
                            <CardHead title="invite a member"/>
                            <CardBody>
                                <div class="d-stack" style="gap:var(--space-8)">
                                    <p>"give them this code. they enter it under profile, family, join a family."</p>
                                    <div class="d-row" style="gap:var(--space-8)">
                                        <code class="d-code heading-lg">{f.invite_code.clone()}</code>
                                        <Button size=Size::Sm on:click=move |_| copy(invite.clone())><Icon name="copy" small=true/>"copy code"</Button>
                                    </div>
                                </div>
                            </CardBody>
                        </Card>
                        <div><Button variant=ButtonVariant::Danger on:click=move |_| confirm.set(true)>"leave family"</Button></div>
                    }.into_any()
                }
            }}
        </div>
        <Dialog open=confirm title="leave family" footer=move || view! {
            <Button on:click=move |_| confirm.set(false)>"cancel"</Button>
            <Button variant=ButtonVariant::Danger on:click=leave>"leave family"</Button>
        }>
            <p>"your accounts become private again, and you drop off joint accounts. nothing is deleted."</p>
        </Dialog>
    }
}

#[component]
fn Connectors() -> impl IntoView {
    let app = expect_context::<AppState>();
    let list = RwSignal::new(Vec::<Connector>::new());
    let name = RwSignal::new(String::new());
    let sc_read = RwSignal::new(true);
    let sc_tx = RwSignal::new(true);
    let sc_add = RwSignal::new(false);
    let sc_edit = RwSignal::new(false);
    let fresh = RwSignal::new(None::<String>);
    let err = RwSignal::new(None::<String>);
    let load = move || {
        leptos::task::spawn_local(async move {
            if let Ok(l) = api::get::<Vec<Connector>>("/connectors").await {
                list.set(l);
            }
        });
    };
    load();
    let origin = web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default();
    let url = format!("{origin}/mcp");
    let create = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let scopes: Vec<&str> = [(sc_read, "read"), (sc_tx, "transactions"), (sc_add, "add"), (sc_edit, "edit")].into_iter().filter(|(s, _)| s.get_untracked()).map(|(_, n)| n).collect();
        let body = serde_json::json!({"name": name.get_untracked(), "scopes": scopes});
        leptos::task::spawn_local(async move {
            match api::post::<CreatedConnector>("/connectors", &body).await {
                Ok(c) => {
                    fresh.set(Some(c.token));
                    name.set(String::new());
                    err.set(None);
                    load();
                }
                Err(e) if e.status == 400 => err.set(Some(e.message)),
                Err(e) => app.fail(&e),
            }
        });
    };
    let revoke = move |id: i64| {
        leptos::task::spawn_local(async move {
            match api::delete(&format!("/connectors/{id}")).await {
                Ok(_) => { app.ok("revoked"); load(); }
                Err(e) => app.fail(&e),
            }
        });
    };
    let url2 = url.clone();
    view! {
        <div class="d-stack" style="gap:var(--space-16);padding-top:var(--space-16)">
            <Card>
                <CardHead title="mcp access" meta="let claude and other tools read and add your money"/>
                <CardBody>
                    <div class="d-stack" style="gap:var(--space-12)">
                        <p>"connect any mcp client to this address. create a token below and send it as a bearer token."</p>
                        <CodeBlock title="mcp endpoint" code=url.clone()/>
                        <CodeBlock title="claude desktop, local (stdio)" code="{\n  \"mcpServers\": {\n    \"tracer\": {\n      \"command\": \"tracer\",\n      \"args\": [\"mcp\"],\n      \"env\": { \"TRACER_TOKEN\": \"<your token>\", \"TRACER_DB\": \"sqlite://tracer.db\" }\n    }\n  }\n}".to_string()/>
                        <span class="d-hint">{format!("remote clients: POST {url2} with Authorization: Bearer <token>")}</span>
                    </div>
                </CardBody>
            </Card>
            <Card>
                <CardHead title="new token"/>
                <CardBody>
                    <form class="d-stack" style="gap:var(--space-12)" on:submit=create>
                        <TextField label="name" value=name placeholder="claude" error=err/>
                        <div class="d-field">
                            <span class="d-label">"it may"</span>
                            <Checkbox checked=sc_read>"read accounts and insights"</Checkbox>
                            <Checkbox checked=sc_tx>"read transactions"</Checkbox>
                            <Checkbox checked=sc_add>"add transactions and transfers"</Checkbox>
                            <Checkbox checked=sc_edit>"edit and delete, and change accounts"</Checkbox>
                        </div>
                        <div><Button variant=ButtonVariant::Primary submit=true>"create token"</Button></div>
                    </form>
                    {move || fresh.get().map(|t| view! {
                        <div class="d-stack" style="gap:var(--space-8);margin-top:var(--space-12)">
                            <StatusChip on=true severity=Severity::Warning>"shown once"</StatusChip>
                            <CodeBlock title="token" code=t/>
                        </div>
                    })}
                </CardBody>
            </Card>
            <Card>
                <CardHead title="tokens"/>
                <CardBody>
                    {move || if list.get().is_empty() { view! { <span class="d-hint">"no tokens yet"</span> }.into_any() } else {
                        view! {
                            <Table label="tokens">
                                <thead><tr><Th>"name"</Th><Th>"may"</Th><Th>"token"</Th><Th>"last used"</Th><Th><span class="d-sr">"revoke"</span></Th></tr></thead>
                                <tbody>
                                    {list.get().into_iter().map(|c| { let id = c.id; let (name, scopes, tail, used, label) = (c.name.clone(), c.scopes.join(", "), format!("…{}", c.tail), c.last_used_at.clone().unwrap_or_else(|| "never".into()), format!("revoke {}", c.name)); view! {
                                        <Tr>
                                            <Td>{name}</Td>
                                            <Td>{scopes}</Td>
                                            <Td>{tail}</Td>
                                            <Td>{used}</Td>
                                            <Td><Button variant=ButtonVariant::Danger size=Size::Sm label=label on:click=move |_| revoke(id)>"revoke"</Button></Td>
                                        </Tr>
                                    } }).collect_view()}
                                </tbody>
                            </Table>
                        }.into_any()
                    }}
                </CardBody>
            </Card>
        </div>
    }
}

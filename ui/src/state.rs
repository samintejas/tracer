use dots_ui::prelude::*;
use leptos::prelude::*;
use tracer_api::*;

use crate::api;

/// What the side panel holds besides a transaction. `None` inside is a new one.
#[derive(Clone)]
pub enum Side {
    Sub(Option<Subscription>),
    Asset(Option<Asset>),
}

/// What every page shares. Pages refetch their own data when `rev` changes; `reload` refreshes the shared
/// parts (who you are, your accounts, tag suggestions, notifications) and bumps it.
#[derive(Clone, Copy)]
pub struct AppState {
    pub me: RwSignal<Option<Me>>,
    pub accounts: RwSignal<Vec<Account>>,
    pub tags: RwSignal<Vec<String>>,
    pub notes: RwSignal<Vec<Notification>>,
    pub rev: RwSignal<u32>,
    /// The transaction open in the side panel.
    pub panel: RwSignal<Option<Transaction>>,
    /// The subscription or asset open in the side panel.
    pub side: RwSignal<Option<Side>>,
    pub toasts: Toasts,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            me: RwSignal::new(None),
            accounts: RwSignal::new(Vec::new()),
            tags: RwSignal::new(Vec::new()),
            notes: RwSignal::new(Vec::new()),
            rev: RwSignal::new(0),
            panel: RwSignal::new(None),
            side: RwSignal::new(None),
            toasts: use_toasts(),
        }
    }

    pub fn currency(&self) -> String {
        self.me.with(|m| m.as_ref().map(|m| m.user.currency.clone()).unwrap_or_else(|| "inr".into()))
    }

    pub fn money(&self, minor: i64) -> String {
        crate::fmt::money(minor, &self.currency())
    }

    pub fn signed(&self, minor: i64) -> String {
        crate::fmt::signed(minor, &self.currency())
    }

    /// Family members (just you when there is no family).
    pub fn members(&self) -> Vec<Member> {
        self.me.with(|m| match m {
            Some(me) => me.family.as_ref().map(|f| f.members.clone()).unwrap_or_else(|| vec![Member { id: me.user.id, name: me.user.name.clone(), initials: me.user.initials.clone(), email: me.user.email.clone() }]),
            None => vec![],
        })
    }

    /// In a family at all (even alone in it).
    pub fn in_family(&self) -> bool {
        self.me.with(|m| m.as_ref().is_some_and(|m| m.family.is_some()))
    }

    /// The family's name, or `personal`.
    pub fn root_name(&self) -> String {
        self.me.with(|m| m.as_ref().and_then(|m| m.family.as_ref().map(|f| f.name.clone())).unwrap_or_else(|| "personal".into()))
    }

    /// More than one person: columns and filters about people make sense.
    pub fn has_family(&self) -> bool {
        self.me.with(|m| m.as_ref().is_some_and(|m| m.family.as_ref().is_some_and(|f| f.members.len() > 1)))
    }

    pub fn my_id(&self) -> i64 {
        self.me.with(|m| m.as_ref().map(|m| m.user.id).unwrap_or(0))
    }

    pub fn account_name(&self, id: i64) -> String {
        self.accounts.with(|a| a.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_else(|| "?".into()))
    }

    /// Accounts you own (the ones you can add to).
    pub fn mine(&self) -> Vec<Account> {
        let me = self.my_id();
        self.accounts.with(|a| a.iter().filter(|a| !a.archived && a.owners.iter().any(|o| o.id == me)).cloned().collect())
    }

    pub fn bump(&self) {
        self.rev.update(|r| *r += 1);
    }

    pub fn ok(&self, text: &str) {
        self.toasts.push(Toast::success(text));
    }

    /// Say what went wrong, in a toast. The text starts with `error:`.
    pub fn error(&self, text: impl Into<String>) {
        let t: String = text.into();
        self.toasts.push(Toast::danger(if t.starts_with("error:") { t } else { format!("error: {t}") }));
    }

    pub fn fail(&self, e: &api::ApiError) {
        if e.unauthorized() {
            api::clear_token();
            self.me.set(None);
        } else {
            self.toasts.push(Toast::danger(format!("error: {}", e.message)));
        }
    }

    /// Refetch the shared parts. A 401 signs you out.
    pub fn reload(&self) {
        let s = *self;
        leptos::task::spawn_local(async move {
            match api::me().await {
                Ok(m) => s.me.set(Some(m)),
                Err(e) => {
                    s.fail(&e);
                    return;
                }
            }
            if let Ok(a) = api::accounts().await {
                s.accounts.set(a);
            }
            if let Ok(t) = api::tags().await {
                s.tags.set(t);
            }
            if let Ok(n) = api::notifications().await {
                s.notes.set(n);
            }
            s.bump();
        });
    }
}

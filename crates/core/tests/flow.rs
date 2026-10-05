use tracer_core::api::*;
use tracer_core::{Caller, Error, Store};

fn acct(name: &str, kind: AccountKind, balance: i64) -> NewAccount {
    NewAccount {
        name: name.into(),
        kind,
        balance: Some(balance),
        visibility: Visibility::Private,
        owner_ids: vec![],
        details: AccountDetails::default(),
    }
}

fn spend(account_id: i64, amount: i64, desc: &str, tags: &[&str]) -> NewTransaction {
    NewTransaction {
        account_id,
        kind: TxKind::Debit,
        amount,
        date: None,
        description: desc.into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        note: String::new(),
    }
}

async fn user(s: &Store, name: &str) -> (Caller, User) {
    let sess = s.sign_up(SignUp { name: name.into(), email: format!("{name}@x.example"), password: "correct horse battery".into() }).await.unwrap();
    (s.authenticate(&sess.token).await.unwrap(), sess.user)
}

#[tokio::test]
async fn auth_and_scopes() {
    let s = Store::memory().await.unwrap();
    let (ana, u) = user(&s, "anita").await;
    assert_eq!(u.initials, "an");
    assert!(matches!(s.sign_in(SignIn { email: "anita@x.example".into(), password: "wrong".into() }).await, Err(Error::Unauthorized)));
    assert!(s.sign_in(SignIn { email: "ANITA@x.example".into(), password: "correct horse battery".into() }).await.is_ok());
    assert!(matches!(s.sign_up(SignUp { name: "x".into(), email: "anita@x.example".into(), password: "correct horse battery".into() }).await, Err(Error::Conflict(_))));
    assert!(matches!(s.sign_up(SignUp { name: "x".into(), email: "x@y.example".into(), password: "too short".into() }).await, Err(Error::BadRequest(_))));

    let bank = s.create_account(&ana, acct("salary", AccountKind::Bank, 100_000_00)).await.unwrap();
    let made = s.create_connector(&ana, NewConnector { name: "claude".into(), scopes: vec!["read".into(), "transactions".into()] }).await.unwrap();
    let ro = s.authenticate(&made.token).await.unwrap();
    assert_eq!(s.accounts(&ro, false).await.unwrap().len(), 1);
    let err = s.add_transaction(&ro, spend(bank.id, 100, "x", &[])).await.unwrap_err();
    assert!(matches!(err, Error::Forbidden(_)), "{err:?}");
    s.revoke_connector(&ana, made.connector.id).await.unwrap();
    assert!(matches!(s.authenticate(&made.token).await, Err(Error::Unauthorized)));
}

#[tokio::test]
async fn balances_transfers_and_filters() {
    let s = Store::memory().await.unwrap();
    let (c, _) = user(&s, "anita").await;
    let bank = s.create_account(&c, acct("salary", AccountKind::Bank, 184_250_00)).await.unwrap();
    let card = s.create_account(&c, acct("card", AccountKind::Credit, 38_420_00)).await.unwrap();
    assert_eq!((bank.balance, card.balance), (184_250_00, 38_420_00));

    s.add_transaction(&c, spend(card.id, 1_980_00, "dinner out", &["Dining"])).await.unwrap();
    assert_eq!(s.account(&c, card.id).await.unwrap().balance, 40_400_00, "a purchase raises what is owed");

    let t = s.transfer(&c, NewTransfer { from_account_id: bank.id, to_account_id: card.id, amount: 40_400_00, date: None, description: String::new(), tags: vec!["transfer".into()], note: String::new() }).await.unwrap();
    assert_eq!(t.len(), 2);
    assert_eq!(s.account(&c, card.id).await.unwrap().balance, 0, "paying the card clears it");
    assert_eq!(s.account(&c, bank.id).await.unwrap().balance, 143_850_00);
    assert_eq!(t[0].counterpart_id, Some(card.id));
    assert_eq!(t[0].description, "card bill, card");
    assert_eq!(t[0].tags, vec!["transfer", "card payment"]);

    s.add_transaction(&c, spend(bank.id, 3_240_00, "groceries, weekly", &["groceries", "household"])).await.unwrap();
    let page = s.transactions(&c, TxFilter { tags: Some("groceries,household".into()), ..Default::default() }).await.unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].tags, vec!["groceries", "household"]);
    assert_eq!(s.transactions(&c, TxFilter { tags: Some("groceries,dining".into()), ..Default::default() }).await.unwrap().total, 0, "tags must all match");
    let all = s.transactions(&c, TxFilter { sort: Some("tag".into()), dir: Some("asc".into()), ..Default::default() }).await.unwrap();
    assert_eq!((all.total_in, all.total_out), (0, 1_980_00 + 3_240_00), "transfers are neither in nor out");
    // money out can be turned into money in
    let flipped = s.update_transaction(&c, page.items[0].id, UpdateTransaction { kind: Some(TxKind::Credit), ..Default::default() }).await.unwrap();
    assert_eq!(flipped.amount, 3_240_00);
    s.update_transaction(&c, flipped.id, UpdateTransaction { kind: Some(TxKind::Debit), ..Default::default() }).await.unwrap();
    let page = s.transactions(&c, TxFilter { kinds: Some("transfer".into()), limit: Some(1), ..Default::default() }).await.unwrap();
    assert_eq!((page.total, page.items.len()), (2, 1));
    let page = s.transactions(&c, TxFilter { q: Some("WEEKLY".into()), sort: Some("amount".into()), dir: Some("asc".into()), ..Default::default() }).await.unwrap();
    assert_eq!(page.total, 1);

    // editing and deleting a transfer touches both legs
    s.update_transaction(&c, t[0].id, UpdateTransaction { amount: Some(100_00), ..Default::default() }).await.unwrap();
    assert_eq!(s.account(&c, card.id).await.unwrap().balance, 40_400_00 - 100_00);
    assert_eq!(s.delete_transaction(&c, t[0].id).await.unwrap(), 2);
    assert_eq!(s.account(&c, card.id).await.unwrap().balance, 40_400_00);

    // setting a balance keeps history
    let a = s.update_account(&c, bank.id, UpdateAccount { balance: Some(50_000_00), ..Default::default() }).await.unwrap();
    assert_eq!(a.balance, 50_000_00);
    assert_eq!(s.transactions(&c, TxFilter { account_id: Some(bank.id), ..Default::default() }).await.unwrap().total, 1);

    let tags = s.tags(&c).await.unwrap();
    assert!(tags.iter().any(|(t, _)| t == "dining"), "{tags:?}");

    // deleting an account takes its transactions; the far side of a transfer stays as plain money
    let t = s.transfer(&c, NewTransfer { from_account_id: bank.id, to_account_id: card.id, amount: 500_00, date: None, description: String::new(), tags: vec![], note: String::new() }).await.unwrap();
    s.delete_account(&c, card.id).await.unwrap();
    let left = s.transaction(&c, t[0].id).await.unwrap();
    assert_eq!((left.kind, left.transfer_id), (TxKind::Debit, None));
    assert_eq!(s.accounts(&c, true).await.unwrap().len(), 1);

    // an untagged entry still gets a group
    let plain = s.add_transaction(&c, spend(bank.id, 10_00, "x", &[])).await.unwrap();
    assert_eq!(plain.tags, vec!["other"]);
}

#[tokio::test]
async fn family_visibility() {
    let s = Store::memory().await.unwrap();
    let (a, au) = user(&s, "anita").await;
    let (v, vu) = user(&s, "vikram").await;
    let (o, _) = user(&s, "outsider").await;

    let secret = s.create_account(&a, acct("salary", AccountKind::Bank, 1_000_00)).await.unwrap();
    assert!(s.create_account(&a, NewAccount { owner_ids: vec![vu.id], ..acct("joint", AccountKind::Bank, 0) }).await.is_err(), "no family yet");

    let fam = s.create_family(&a, NewFamily { name: "Rao Family".into() }).await.unwrap();
    assert_eq!(fam.name, "rao family");
    assert!(fam.invite_code.is_none(), "no code until the owner asks for one");
    assert!(matches!(s.join_family(&v, JoinFamily { code: "NOPE-NOPE".into() }).await, Err(Error::BadRequest(_))));
    let code = s.generate_invite(&a).await.unwrap().invite_code.unwrap();
    let joined = s.join_family(&v, JoinFamily { code: code.to_lowercase() }).await.unwrap();
    assert_eq!(joined.owner_id, au.id);
    assert!(joined.invite_code.is_none());
    assert!(matches!(s.join_family(&o, JoinFamily { code }).await, Err(Error::BadRequest(_))), "a code works once");
    assert!(matches!(s.generate_invite(&v).await, Err(Error::Forbidden(_))), "members cannot invite");
    assert!(s.notifications(&a).await.unwrap()[0].title.contains("joined"));

    let shared = s.create_account(&a, NewAccount { visibility: Visibility::Shared, ..acct("savings", AccountKind::Bank, 5_000_00) }).await.unwrap();
    let joint = s.create_account(&a, NewAccount { owner_ids: vec![vu.id], ..acct("household joint", AccountKind::Bank, 2_000_00) }).await.unwrap();
    assert!(joint.joint && joint.owners.len() == 2);

    let names = |list: Vec<Account>| list.into_iter().map(|a| a.name).collect::<Vec<_>>();
    assert_eq!(names(s.accounts(&v, false).await.unwrap()), vec!["savings", "household joint"], "private stays private");
    assert_eq!(s.accounts(&o, false).await.unwrap().len(), 0);
    assert!(matches!(s.account(&v, secret.id).await, Err(Error::NotFound(_))));

    // shared is visible but not writable by the partner; joint is writable by both
    assert!(matches!(s.add_transaction(&v, spend(shared.id, 10_00, "x", &[])).await, Err(Error::Forbidden(_))));
    s.add_transaction(&v, spend(joint.id, 2_860_00, "electricity bill", &["utilities"])).await.unwrap();
    assert!(s.notifications(&a).await.unwrap()[0].title.contains("vikram spent"));
    assert_eq!(s.transactions(&a, TxFilter::default()).await.unwrap().total, 1);
    assert_eq!(s.transactions(&o, TxFilter::default()).await.unwrap().total, 0);

    let ins = s.insights(&a, InsightsQuery { member_id: Some(vu.id), days: None }).await.unwrap();
    assert_eq!(ins.spending, 2_860_00);
    assert_eq!(ins.categories[0].tag, "utilities");
    let me = s.me(&a).await.unwrap();
    assert_eq!(me.family.unwrap().members.len(), 2);
    assert_eq!(me.user.id, au.id);

    assert!(matches!(s.delete_family(&v).await, Err(Error::Forbidden(_))));
    s.leave_family(&v).await.unwrap();
    assert_eq!(s.accounts(&v, false).await.unwrap().len(), 0, "leaver loses the joint account and the shared view");
    s.delete_family(&a).await.unwrap();
    assert!(s.me(&a).await.unwrap().family.is_none());
    assert_eq!(s.accounts(&a, false).await.unwrap().len(), 3, "the owner keeps what they own");
}

#[tokio::test]
async fn loans_dues_and_ask() {
    let s = Store::memory().await.unwrap();
    let (c, _) = user(&s, "anita").await;
    assert!(s.create_account(&c, acct("home loan", AccountKind::Loan, 0)).await.is_err(), "a loan needs its terms");
    let now = chrono::Local::now().date_naive();
    let loan = s
        .create_account(&c, NewAccount {
            details: AccountDetails { loan_total: Some(2_500_000_00), rate: Some(8.5), tenure: Some(180), start: Some("2019-04".into()), emi_day: Some(5), ..Default::default() },
            ..acct("home loan", AccountKind::Loan, 0)
        })
        .await
        .unwrap();
    let l = loan.loan.as_ref().unwrap();
    assert!(l.left < 180 && loan.balance == l.balance);
    let card = s
        .create_account(&c, NewAccount { details: AccountDetails { due_day: Some(18), limit: Some(300_000_00), ..Default::default() }, ..acct("card", AccountKind::Credit, 38_420_00) })
        .await
        .unwrap();
    s.add_transaction(&c, spend(card.id, 500_00, "groceries", &["groceries"])).await.unwrap();
    let ins = s.insights(&c, InsightsQuery::default()).await.unwrap();
    assert_eq!(ins.dues.len(), 2);
    assert!(ins.dues[0].date >= now.to_string());
    assert_eq!(ins.owed, loan.balance + 38_920_00);
    assert_eq!(ins.months.len(), 6);
    assert_eq!(ins.months.last().unwrap().spending, 500_00);
    assert!((22..=28).contains(&ins.days.len()));
    // the card is due on the 18th: a reminder appears only within three days of it
    let notes = s.notifications(&c).await.unwrap();
    let due_soon = (chrono::NaiveDate::parse_from_str(&ins.dues.iter().find(|d| d.account_id == card.id).unwrap().date, "%Y-%m-%d").unwrap() - now).num_days() <= 3;
    assert_eq!(notes.iter().any(|n| n.title.starts_with("card payment due")), due_soon);
    assert_eq!(s.notifications(&c).await.unwrap().len(), notes.len(), "a reminder is made once");

    let a = s.ask(&c, "when do my loans end?").await.unwrap();
    assert!(a.contains("home loan") && a.contains("months left"), "{a}");
    let a = s.ask(&c, "how much did i spend on groceries?").await.unwrap();
    assert!(a.contains("₹500") && a.contains("1 transaction."), "{a}");
    assert!(s.ask(&c, "what is due next?").await.unwrap().contains("card"));
}

#[tokio::test]
async fn attachments() {
    let s = Store::memory().await.unwrap();
    let (c, _) = user(&s, "anita").await;
    let bank = s.create_account(&c, acct("salary", AccountKind::Bank, 0)).await.unwrap();
    let tx = s.add_transaction(&c, spend(bank.id, 10_00, "x", &[])).await.unwrap();
    let a = s.add_attachment(&c, tx.id, "../../receipt.jpg", "image/jpeg", vec![1, 2, 3]).await.unwrap();
    assert_eq!(a.name, "receipt.jpg");
    assert_eq!(s.transaction(&c, tx.id).await.unwrap().attachments.len(), 1);
    assert_eq!(s.attachment(&c, a.id).await.unwrap().2, vec![1, 2, 3]);
    s.delete_attachment(&c, a.id).await.unwrap();
    assert!(s.transaction(&c, tx.id).await.unwrap().attachments.is_empty());
}

#[tokio::test]
async fn profile_sessions_and_deleting_yourself() {
    let s = Store::memory().await.unwrap();
    let sess = s.sign_up(SignUp { name: "anita rao".into(), email: "a@x.example".into(), password: "correct horse battery".into() }).await.unwrap();
    let c = s.authenticate(&sess.token).await.unwrap();
    let u = s.update_profile(&c, UpdateProfile { picture: Some("data:image/png;base64,AAAA".into()), notify_joint: Some(false), currency: Some("usd".into()), ..Default::default() }).await.unwrap();
    assert!(u.picture.is_some() && !u.notify_joint && u.notify_card && u.currency == "usd");
    assert!(s.update_profile(&c, UpdateProfile { picture: Some("javascript:alert(1)".into()), ..Default::default() }).await.is_err());
    assert!(s.update_profile(&c, UpdateProfile { picture: Some(String::new()), ..Default::default() }).await.unwrap().picture.is_none());

    let bank = s.create_account(&c, acct("salary", AccountKind::Bank, 100_00)).await.unwrap();
    s.add_transaction(&c, spend(bank.id, 10_00, "x", &["a"])).await.unwrap();
    let csv = s.export_csv(&c).await.unwrap();
    assert!(csv.contains("salary") && csv.contains("-10.00"), "{csv}");

    let made = s.create_connector(&c, NewConnector { name: "mcp".into(), scopes: vec!["read".into()] }).await.unwrap();
    let up = s.update_connector(&c, made.connector.id, UpdateConnector { scopes: vec!["read".into(), "add".into()] }).await.unwrap();
    assert_eq!(up.scopes, vec!["read", "add"]);

    s.sign_out_all(&c).await.unwrap();
    assert!(matches!(s.authenticate(&sess.token).await, Err(Error::Unauthorized)));
    assert!(s.authenticate(&made.token).await.is_ok(), "connector tokens survive a sign-out");

    assert!(matches!(s.delete_user(&c, "wrong").await, Err(Error::Forbidden(_))));
    s.delete_user(&c, "correct horse battery").await.unwrap();
    assert!(matches!(s.sign_in(SignIn { email: "a@x.example".into(), password: "correct horse battery".into() }).await, Err(Error::Unauthorized)));
}

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
    let sess = s.sign_up(SignUp { name: name.into(), email: format!("{name}@x.example"), password: "correct horse".into() }).await.unwrap();
    (s.authenticate(&sess.token).await.unwrap(), sess.user)
}

#[tokio::test]
async fn auth_and_scopes() {
    let s = Store::memory().await.unwrap();
    let (ana, u) = user(&s, "anita").await;
    assert_eq!(u.initials, "an");
    assert!(matches!(s.sign_in(SignIn { email: "anita@x.example".into(), password: "wrong".into() }).await, Err(Error::Unauthorized)));
    assert!(s.sign_in(SignIn { email: "ANITA@x.example".into(), password: "correct horse".into() }).await.is_ok());
    assert!(matches!(s.sign_up(SignUp { name: "x".into(), email: "anita@x.example".into(), password: "correct horse".into() }).await, Err(Error::Conflict(_))));
    assert!(matches!(s.sign_up(SignUp { name: "x".into(), email: "x@y.example".into(), password: "short".into() }).await, Err(Error::BadRequest(_))));

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

    s.add_transaction(&c, spend(bank.id, 3_240_00, "groceries, weekly", &["groceries", "household"])).await.unwrap();
    let page = s.transactions(&c, TxFilter { tags: Some("groceries".into()), ..Default::default() }).await.unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].tags, vec!["groceries", "household"]);
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
    assert!(matches!(s.join_family(&v, JoinFamily { code: "NOPE-NOPE".into() }).await, Err(Error::BadRequest(_))));
    s.join_family(&v, JoinFamily { code: fam.invite_code.to_lowercase() }).await.unwrap();
    assert!(s.notifications(&a).await.unwrap()[0].text.contains("joined"));

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
    assert!(s.notifications(&a).await.unwrap()[0].text.contains("vikram spent"));
    assert_eq!(s.transactions(&a, TxFilter::default()).await.unwrap().total, 1);
    assert_eq!(s.transactions(&o, TxFilter::default()).await.unwrap().total, 0);

    let ins = s.insights(&a, InsightsQuery { member_id: Some(vu.id), days: None }).await.unwrap();
    assert_eq!(ins.spending, 2_860_00);
    assert_eq!(ins.categories[0].tag, "utilities");
    let me = s.me(&a).await.unwrap();
    assert_eq!(me.family.unwrap().members.len(), 2);
    assert_eq!(me.user.id, au.id);

    s.leave_family(&v).await.unwrap();
    assert_eq!(s.accounts(&v, false).await.unwrap().len(), 0, "leaver loses the joint account and the shared view");
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
    assert_eq!(ins.days.len(), 28);

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

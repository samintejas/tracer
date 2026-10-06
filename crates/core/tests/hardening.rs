//! One test per rule that was once broken, so it stays fixed.

use pebblelab_core::api::*;
use pebblelab_core::{Caller, Config, Error, ExternalIdentity, Store};

fn acct(name: &str, kind: AccountKind, balance: i64) -> NewAccount {
    NewAccount { name: name.into(), kind, asset_id: None, balance: Some(balance), visibility: Visibility::Private, owner_ids: vec![], details: AccountDetails::default() }
}

fn spend(account_id: i64, amount: i64, desc: &str) -> NewTransaction {
    NewTransaction { account_id, kind: TxKind::Debit, amount, date: None, description: desc.into(), tags: vec![], note: String::new() }
}

async fn user(s: &Store, name: &str) -> (Caller, User) {
    let sess = s.sign_up(SignUp { name: name.into(), email: format!("{name}@x.example"), password: "correct horse battery".into() }).await.unwrap();
    (s.authenticate(&sess.token).await.unwrap(), sess.user)
}

/// Two people in one family, and a joint bank account between them.
async fn pair(s: &Store) -> (Caller, User, Caller, User, Account) {
    let (a, au) = user(s, "anita").await;
    let (v, vu) = user(s, "vikram").await;
    s.create_family(&a, NewFamily { name: "rao".into() }).await.unwrap();
    let code = s.generate_invite(&a).await.unwrap().invite_code.unwrap();
    s.join_family(&v, JoinFamily { code }).await.unwrap();
    let joint = s.create_account(&a, NewAccount { owner_ids: vec![vu.id], ..acct("joint", AccountKind::Bank, 1_000_00) }).await.unwrap();
    (a, au, v, vu, joint)
}

#[tokio::test]
async fn a_token_that_may_only_add_can_add() {
    let s = Store::test().await.unwrap();
    let (c, _) = user(&s, "anita").await;
    let bank = s.create_account(&c, acct("salary", AccountKind::Bank, 100_00)).await.unwrap();
    let other = s.create_account(&c, acct("savings", AccountKind::Bank, 0)).await.unwrap();
    let made = s.create_connector(&c, NewConnector { name: "logger".into(), scopes: vec!["add".into()] }).await.unwrap();
    let w = s.authenticate(&made.token).await.unwrap();
    let t = s.add_transaction(&w, spend(bank.id, 10_00, "tea")).await.expect("the write succeeds and so does its reply");
    assert_eq!(t.amount, -10_00);
    s.transfer(&w, NewTransfer { from_account_id: bank.id, to_account_id: other.id, amount: 5_00, description: String::new(), tags: vec![], note: String::new(), date: None }).await.unwrap();
    assert!(matches!(s.transactions(&w, TxFilter::default()).await, Err(Error::Forbidden(_))), "reading still needs its scope");
    assert_eq!(s.transactions(&c, TxFilter::default()).await.unwrap().total, 3);
}

#[tokio::test]
async fn an_invite_works_once_even_when_two_people_race_and_expires() {
    let s = Store::test().await.unwrap();
    let (a, _) = user(&s, "anita").await;
    let (v, _) = user(&s, "vikram").await;
    let (w, _) = user(&s, "wanda").await;
    s.create_family(&a, NewFamily { name: "rao".into() }).await.unwrap();
    let code = s.generate_invite(&a).await.unwrap().invite_code.unwrap();
    let (r1, r2) = tokio::join!(s.join_family(&v, JoinFamily { code: code.clone() }), s.join_family(&w, JoinFamily { code: code.clone() }));
    assert_eq!([r1.is_ok(), r2.is_ok()].iter().filter(|ok| **ok).count(), 1, "{r1:?} {r2:?}");
    assert_eq!(s.me(&a).await.unwrap().family.unwrap().members.len(), 2);

    let late = s.generate_invite(&a).await.unwrap().invite_code.unwrap();
    s.raw("UPDATE families SET invite_expires = '2000-01-01 00:00:00'").await.unwrap();
    let (x, _) = user(&s, "xavier").await;
    assert!(matches!(s.join_family(&x, JoinFamily { code: late }).await, Err(Error::BadRequest(_))), "an old code is dead");
    assert!(s.me(&a).await.unwrap().family.unwrap().invite_code.is_none(), "and the owner is not shown it");
}

#[tokio::test]
async fn deleting_yourself_leaves_a_joint_account_to_the_other_owner() {
    let s = Store::test().await.unwrap();
    let (a, _, v, _, joint) = pair(&s).await;
    s.add_transaction(&v, spend(joint.id, 10_00, "bill")).await.unwrap();
    let mine = s.create_account(&a, acct("only mine", AccountKind::Bank, 5_00)).await.unwrap();
    s.delete_family(&a).await.unwrap();
    assert_eq!(s.account(&v, joint.id).await.unwrap().owners.len(), 2, "the family going does not end joint ownership");
    s.delete_user(&a, "correct horse battery").await.unwrap();
    let left = s.account(&v, joint.id).await.expect("the joint account survives");
    assert_eq!(left.owners.len(), 1);
    assert_eq!(left.balance, 1_000_00 - 10_00, "with its history");
    assert_eq!(s.transactions(&v, TxFilter::default()).await.unwrap().total, 1);
    assert!(s.account(&v, mine.id).await.is_err(), "what only they owned goes with them");
}

#[tokio::test]
async fn joint_accounts_cannot_be_taken_from_the_other_owner() {
    let s = Store::test().await.unwrap();
    let (a, _, v, vu, joint) = pair(&s).await;
    // anita cannot drop vikram, and cannot delete the account while he is on it
    let err = s.update_account(&a, joint.id, UpdateAccount { owner_ids: Some(vec![]), ..Default::default() }).await.unwrap_err();
    assert!(matches!(err, Error::Forbidden(_)), "{err:?}");
    assert!(matches!(s.delete_account(&a, joint.id).await, Err(Error::Conflict(_))));
    assert_eq!(s.account(&v, joint.id).await.unwrap().owners.len(), 2);
    // renaming while naming the same owners is fine
    s.update_account(&a, joint.id, UpdateAccount { name: Some("home".into()), owner_ids: Some(vec![vu.id]), ..Default::default() }).await.unwrap();
    // he can leave, and then she may delete it
    s.leave_account(&v, joint.id).await.unwrap();
    assert!(s.account(&v, joint.id).await.is_err());
    assert!(matches!(s.leave_account(&a, joint.id).await, Err(Error::Conflict(_))), "the last owner cannot leave");
    s.delete_account(&a, joint.id).await.unwrap();
}

#[tokio::test]
async fn transfers_stay_between_your_own_accounts_and_edits_are_all_or_nothing() {
    let s = Store::test().await.unwrap();
    let (a, _, v, _, joint) = pair(&s).await;
    let shared = s.create_account(&v, NewAccount { visibility: Visibility::Shared, ..acct("vikram savings", AccountKind::Bank, 0) }).await.unwrap();
    let mine = s.create_account(&a, acct("salary", AccountKind::Bank, 500_00)).await.unwrap();
    let to_theirs = NewTransfer { from_account_id: mine.id, to_account_id: shared.id, amount: 10_00, description: String::new(), tags: vec![], note: String::new(), date: None };
    assert!(matches!(s.transfer(&a, to_theirs).await, Err(Error::Forbidden(_))), "no putting money into an account you do not own");
    assert_eq!(s.account(&a, mine.id).await.unwrap().balance, 500_00, "and nothing was half done");

    let legs = s.transfer(&a, NewTransfer { from_account_id: mine.id, to_account_id: joint.id, amount: 50_00, description: String::new(), tags: vec![], note: String::new(), date: None }).await.unwrap();
    // a refused edit changes nothing
    let bad = UpdateTransaction { amount: Some(99_00), kind: Some(TxKind::Debit), ..Default::default() };
    assert!(s.update_transaction(&a, legs[0].id, bad).await.is_err());
    assert_eq!(s.transaction(&a, legs[0].id).await.unwrap().amount, -50_00);
    assert_eq!(s.transaction(&a, legs[1].id).await.unwrap().amount, 50_00);
    // an edit to a transfer moves both legs together
    s.update_transaction(&a, legs[0].id, UpdateTransaction { amount: Some(60_00), ..Default::default() }).await.unwrap();
    assert_eq!(s.transaction(&a, legs[1].id).await.unwrap().amount, 60_00);
}

#[tokio::test]
async fn the_csv_cannot_run_a_formula() {
    let s = Store::test().await.unwrap();
    let (c, _) = user(&s, "anita").await;
    let bank = s.create_account(&c, acct("salary", AccountKind::Bank, 0)).await.unwrap();
    s.add_transaction(&c, spend(bank.id, 1_00, "=HYPERLINK(\"http://evil\",\"x\")")).await.unwrap();
    s.add_transaction(&c, spend(bank.id, 2_00, "@cmd")).await.unwrap();
    let csv = s.export_csv(&c).await.unwrap();
    assert!(!csv.lines().any(|l| l.contains(",=") || l.contains(",@") || l.starts_with("=")), "{csv}");
    assert!(csv.contains("'@cmd") && csv.contains("'=HYPERLINK"), "{csv}");
    assert!(csv.contains("-2.00"), "amounts stay plain numbers");
}

#[tokio::test]
async fn uploaded_files_can_never_be_a_page() {
    let s = Store::test().await.unwrap();
    let (c, _) = user(&s, "anita").await;
    let bank = s.create_account(&c, acct("salary", AccountKind::Bank, 0)).await.unwrap();
    let tx = s.add_transaction(&c, spend(bank.id, 1_00, "x")).await.unwrap();
    let html = s.add_attachment(&c, tx.id, "r.html", "text/html; charset=utf-8", b"<script>1</script>".to_vec()).await.unwrap();
    let svg = s.add_attachment(&c, tx.id, "r.svg", "image/svg+xml", b"<svg/>".to_vec()).await.unwrap();
    let png = s.add_attachment(&c, tx.id, "r.png", "IMAGE/PNG", vec![1]).await.unwrap();
    assert_eq!(s.attachment(&c, html.id).await.unwrap().1, "application/octet-stream");
    assert_eq!(s.attachment(&c, svg.id).await.unwrap().1, "application/octet-stream");
    assert_eq!(s.attachment(&c, png.id).await.unwrap().1, "image/png");
    // a type stored before this rule is cleaned on the way out too
    s.raw(&format!("UPDATE attachments SET mime = 'text/html' WHERE id = {}", png.id)).await.unwrap();
    assert_eq!(s.attachment(&c, png.id).await.unwrap().1, "application/octet-stream");
    for _ in 0..5 {
        s.add_attachment(&c, tx.id, "f", "image/png", vec![1]).await.unwrap();
    }
    assert!(s.add_attachment(&c, tx.id, "one too many", "image/png", vec![1]).await.is_err());
}

#[tokio::test]
async fn sign_ups_can_be_closed_but_the_cli_can_still_add_people() {
    let s = Store::test().await.unwrap().with_config(Config { signups_open: false, ..Config::default() });
    let b = || SignUp { name: "anita".into(), email: "a@x.example".into(), password: "correct horse battery".into() };
    assert!(matches!(s.sign_up(b()).await, Err(Error::Forbidden(_))));
    s.create_user(b()).await.unwrap();
    assert!(s.sign_in(SignIn { email: "a@x.example".into(), password: "correct horse battery".into() }).await.is_ok());
}

#[tokio::test]
async fn sessions_end_after_a_month_idle_but_not_while_in_use() {
    let s = Store::test().await.unwrap();
    let sess = s.sign_up(SignUp { name: "anita".into(), email: "a@x.example".into(), password: "correct horse battery".into() }).await.unwrap();
    s.raw("UPDATE tokens SET created_at = utc_text(now() - interval '40 days'), last_used_at = utc_text(now() - interval '5 days')").await.unwrap();
    assert!(s.authenticate(&sess.token).await.is_ok(), "used 5 days ago, so still good");
    s.raw("UPDATE tokens SET last_used_at = utc_text(now() - interval '31 days')").await.unwrap();
    assert!(matches!(s.authenticate(&sess.token).await, Err(Error::Unauthorized)), "idle for over a month");
    s.raw("UPDATE tokens SET created_at = utc_text(now() - interval '200 days'), last_used_at = utc_text(now())").await.unwrap();
    assert!(matches!(s.authenticate(&sess.token).await, Err(Error::Unauthorized)), "no session lasts past 180 days");
    let r = s.run_jobs().await.unwrap();
    assert!(r.pruned >= 1, "the dead session is removed");
}

#[tokio::test]
async fn the_background_job_posts_renewals_for_everyone_and_reads_do_not_write() {
    let s = Store::test().await.unwrap();
    let (a, _, v, _, _) = pair(&s).await;
    let shared = s.create_account(&a, NewAccount { visibility: Visibility::Shared, ..acct("household", AccountKind::Bank, 1_000_00) }).await.unwrap();
    let due = (chrono::Utc::now().date_naive() - chrono::Duration::days(1)).to_string();
    // a renewal that is already due is posted when the subscription is saved ...
    s.add_subscription(&a, NewSubscription { name: "internet".into(), amount: 100_00, cycle: Cycle::Monthly, next: Some(due.clone()), account_id: shared.id, tag: String::new(), active: true }).await.unwrap();
    // ... and one made due afterwards waits for the job, whoever reads in between
    let id = s.subscriptions(&a).await.unwrap()[0].id;
    s.raw(&format!("UPDATE subscriptions SET next_on = '{due}' WHERE id = {id}")).await.unwrap();
    let before = s.account(&v, shared.id).await.unwrap().balance;
    assert_eq!(s.account(&v, shared.id).await.unwrap().balance, before, "reading changes nothing");
    let r = s.run_jobs().await.unwrap();
    assert_eq!(r.renewals, 1);
    assert_eq!(s.account(&v, shared.id).await.unwrap().balance, before - 100_00, "and the partner sees it without the owner opening the app");
    assert_eq!(s.run_jobs().await.unwrap().renewals, 0, "once only");
}

#[tokio::test]
async fn today_follows_the_configured_zone() {
    let s = Store::test().await.unwrap().with_config(Config { tz: "Pacific/Kiritimati".parse().unwrap(), ..Config::default() });
    let (c, _) = user(&s, "anita").await;
    let bank = s.create_account(&c, acct("salary", AccountKind::Bank, 0)).await.unwrap();
    let t = s.add_transaction(&c, spend(bank.id, 1_00, "x")).await.unwrap();
    let kiritimati = chrono::Utc::now().with_timezone(&"Pacific/Kiritimati".parse::<chrono_tz::Tz>().unwrap()).date_naive();
    assert_eq!(t.date, kiritimati.to_string());
}

// ---- signing in with a provider ---------------------------------------------------------------------

fn google(subject: &str, email: &str, verified: bool) -> ExternalIdentity {
    ExternalIdentity { provider: "google".into(), subject: subject.into(), email: email.into(), email_verified: verified, name: "Anita Rao".into(), picture: None }
}

#[tokio::test]
async fn a_provider_sign_in_makes_an_account_once_and_finds_it_again() {
    let s = Store::test().await.unwrap();
    let code = s.external_sign_in(google("g-1", "Anita@X.example", true)).await.unwrap();
    let first = s.redeem_login_code(&code).await.unwrap();
    assert_eq!((first.user.email.as_str(), first.user.name.as_str()), ("anita@x.example", "anita rao"));
    assert!(matches!(s.redeem_login_code(&code).await, Err(Error::Unauthorized)), "a code works once");
    // the same person again, even with another address at the provider
    let again = s.redeem_login_code(&s.external_sign_in(google("g-1", "other@x.example", true)).await.unwrap()).await.unwrap();
    assert_eq!(again.user.id, first.user.id);
    assert_eq!(s.identities(first.user.id).await.unwrap(), vec!["google"]);
    // an account that only ever used a provider has no password to guess
    assert!(matches!(s.sign_in(SignIn { email: "anita@x.example".into(), password: "!".into() }).await, Err(Error::Unauthorized)));
    assert!(matches!(s.sign_in(SignIn { email: "anita@x.example".into(), password: String::new() }).await, Err(Error::Unauthorized)));
}

#[tokio::test]
async fn a_provider_picture_fills_a_gap_but_never_replaces_the_persons_own() {
    let s = Store::test().await.unwrap();
    let with = |pic: &str| ExternalIdentity { picture: Some(pic.into()), ..google("g-1", "a@x.example", true) };
    let me = s.redeem_login_code(&s.external_sign_in(with("data:image/png;base64,AAAA")).await.unwrap()).await.unwrap().user;
    assert_eq!(me.picture.as_deref(), Some("data:image/png;base64,AAAA"));
    let c = Caller::full(me.id);
    s.update_profile(&c, UpdateProfile { picture: Some("data:image/png;base64,MINE".into()), ..Default::default() }).await.unwrap();
    s.redeem_login_code(&s.external_sign_in(with("data:image/png;base64,BBBB")).await.unwrap()).await.unwrap();
    assert_eq!(s.user(me.id).await.unwrap().picture.as_deref(), Some("data:image/png;base64,MINE"));
    // anything that is not an inline image is ignored
    let t = Store::test().await.unwrap();
    let bad = ExternalIdentity { picture: Some("https://evil.example/x.png".into()), ..google("g-2", "b@x.example", true) };
    assert_eq!(t.redeem_login_code(&t.external_sign_in(bad).await.unwrap()).await.unwrap().user.picture, None);
}

#[tokio::test]
async fn a_provider_must_vouch_for_the_email_before_it_is_trusted() {
    let s = Store::test().await.unwrap();
    assert!(matches!(s.external_sign_in(google("g-1", "a@x.example", false)).await, Err(Error::Forbidden(_))));
    assert!(matches!(s.external_sign_in(google("g-1", "", true)).await, Err(Error::BadRequest(_))));
    assert!(matches!(s.external_sign_in(ExternalIdentity { provider: "myspace".into(), ..google("g-1", "a@x.example", true) }).await, Err(Error::NotFound(_))));
    // nothing was made along the way
    s.sign_up(SignUp { name: "anita".into(), email: "a@x.example".into(), password: "correct horse battery".into() }).await.expect("the address is still free");
}

#[tokio::test]
async fn linking_an_existing_account_cuts_off_whoever_set_it_up_with_a_password() {
    let s = Store::test().await.unwrap();
    // someone signs up with an address that is not theirs; nothing ever checked it
    let squatter = s.sign_up(SignUp { name: "squatter".into(), email: "anita@x.example".into(), password: "correct horse battery".into() }).await.unwrap();
    let squat = s.authenticate(&squatter.token).await.unwrap();
    // the real owner signs in with the provider, which does vouch for the address
    let owner = s.redeem_login_code(&s.external_sign_in(google("g-1", "anita@x.example", true)).await.unwrap()).await.unwrap();
    assert_eq!(owner.user.id, squatter.user.id, "same account, now tied to the provider");
    assert!(matches!(s.authenticate(&squatter.token).await, Err(Error::Unauthorized)), "the old session is gone");
    assert!(matches!(s.sign_in(SignIn { email: "anita@x.example".into(), password: "correct horse battery".into() }).await, Err(Error::Unauthorized)), "and so is the old password");
    let _ = squat;
    // the owner can set a password of their own, signed in, with nothing to quote
    let me = s.authenticate(&owner.token).await.unwrap();
    s.change_password(&me, ChangePassword { current: String::new(), new: "a brand new passphrase".into() }).await.unwrap();
    assert!(s.sign_in(SignIn { email: "anita@x.example".into(), password: "a brand new passphrase".into() }).await.is_ok());
}

#[tokio::test]
async fn an_api_token_cannot_set_a_password_on_a_provider_only_account() {
    let s = Store::test().await.unwrap();
    let owner = s.redeem_login_code(&s.external_sign_in(google("g-1", "anita@x.example", true)).await.unwrap()).await.unwrap();
    let me = s.authenticate(&owner.token).await.unwrap();
    let made = s.create_connector(&me, NewConnector { name: "script".into(), scopes: vec!["read".into(), "edit".into()] }).await.unwrap();
    let token = s.authenticate(&made.token).await.unwrap();
    let err = s.change_password(&token, ChangePassword { current: String::new(), new: "a brand new passphrase".into() }).await.unwrap_err();
    assert!(matches!(err, Error::Forbidden(_)), "{err:?}");
}

#[tokio::test]
async fn closed_sign_ups_stop_new_provider_accounts_but_not_people_already_in() {
    let s = Store::test().await.unwrap();
    s.external_sign_in(google("g-1", "anita@x.example", true)).await.unwrap();
    let s = s.with_config(Config { signups_open: false, ..Config::default() });
    assert!(matches!(s.external_sign_in(google("g-2", "stranger@x.example", true)).await, Err(Error::Forbidden(_))));
    assert!(s.external_sign_in(google("g-1", "anita@x.example", true)).await.is_ok(), "an existing member still signs in");
}

#[tokio::test]
async fn provider_flows_and_login_codes_work_once_and_expire() {
    let s = Store::test().await.unwrap();
    let (state, verifier) = s.oauth_start("github").await.unwrap();
    assert!(matches!(s.oauth_finish("google", &state).await, Err(Error::BadRequest(_))), "not for another provider");
    assert_eq!(s.oauth_finish("github", &state).await.unwrap(), verifier);
    assert!(matches!(s.oauth_finish("github", &state).await, Err(Error::BadRequest(_))), "a second return with the same state");
    let (old, _) = s.oauth_start("github").await.unwrap();
    s.raw("UPDATE oauth_flows SET created_at = utc_text(now() - interval '11 minutes')").await.unwrap();
    assert!(matches!(s.oauth_finish("github", &old).await, Err(Error::BadRequest(_))), "ten minutes is the limit");
    assert!(matches!(s.oauth_start("myspace").await, Err(Error::NotFound(_))));

    let code = s.external_sign_in(google("g-1", "anita@x.example", true)).await.unwrap();
    s.raw("UPDATE login_codes SET created_at = utc_text(now() - interval '3 minutes')").await.unwrap();
    assert!(matches!(s.redeem_login_code(&code).await, Err(Error::Unauthorized)), "a code lasts two minutes");
    assert!(matches!(s.redeem_login_code("lgc_guess").await, Err(Error::Unauthorized)));
}

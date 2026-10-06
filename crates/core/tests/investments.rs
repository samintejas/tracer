//! Investment types, the details each asks for, and investments that are also assets.

use pebblelab_core::api::*;
use pebblelab_core::{Caller, Store};

fn invest(name: &str, kind: &str, details: AccountDetails) -> NewAccount {
    NewAccount {
        name: name.into(),
        kind: AccountKind::Investment,
        asset_id: None,
        balance: None,
        visibility: Visibility::Private,
        owner_ids: vec![],
        details: AccountDetails { invest_kind: kind.into(), ..details },
    }
}

async fn user(s: &Store, name: &str) -> Caller {
    let sess = s.sign_up(SignUp { name: name.into(), email: format!("{name}@x.example"), password: "correct horse battery".into() }).await.unwrap();
    s.authenticate(&sess.token).await.unwrap()
}

fn gold(value: i64) -> NewAsset {
    NewAsset { name: "gold coins".into(), kind: "gold".into(), bought: String::new(), cost: Some(value / 2), value, note: String::new() }
}

fn month_now() -> String {
    chrono::Utc::now().format("%Y-%m").to_string()
}

#[tokio::test]
async fn only_known_types_and_complete_deposits() {
    let s = Store::test().await.unwrap();
    let c = user(&s, "anita").await;
    let err = s.create_account(&c, invest("x", "lottery tickets", AccountDetails::default())).await.unwrap_err();
    assert!(err.to_string().contains("investment type must be one of"), "{err}");
    // no type means other
    let a = s.create_account(&c, invest("misc", "", AccountDetails::default())).await.unwrap();
    assert_eq!(a.details.invest_kind, "other");
    // a fixed deposit needs its terms
    assert!(s.create_account(&c, invest("fd", "fixed deposit", AccountDetails { invested: Some(1000_00), ..Default::default() })).await.is_err());
    // a recurring one needs the instalment, not a lump sum
    assert!(s.create_account(&c, invest("rd", "recurring deposit", AccountDetails { invested: Some(1000_00), rate: Some(6.5), tenure: Some(12), start: Some(month_now()), ..Default::default() })).await.is_err());
    // a market investment needs nothing more
    s.create_account(&c, invest("index", "mutual fund", AccountDetails { invested: Some(10_000_00), ..Default::default() })).await.unwrap();
}

#[tokio::test]
async fn a_deposit_knows_what_it_is_worth() {
    let s = Store::test().await.unwrap();
    let c = user(&s, "anita").await;
    let fd = s
        .create_account(&c, invest("bank fd", "fixed deposit", AccountDetails { invested: Some(100_000_00), rate: Some(7.2), tenure: Some(12), start: Some(month_now()), ..Default::default() }))
        .await
        .unwrap();
    let calc = fd.deposit.as_ref().expect("a deposit with its terms gets worked out");
    assert_eq!(calc.invested, 100_000_00);
    assert_eq!(calc.months_left, 12);
    assert!((calc.maturity_value - 107_399_00).abs() < 100_00, "{}", calc.maturity_value);
    // left blank, the balance starts at what the terms say it is worth today (just opened: what went in)
    assert_eq!(fd.balance, 100_000_00);
    // a figure from the bank's statement wins
    let set = s.update_account(&c, fd.id, UpdateAccount { balance: Some(100_500_00), ..Default::default() }).await.unwrap();
    assert_eq!(set.balance, 100_500_00);
    let answer = s.ask(&c, "when does my fixed deposit mature?").await.unwrap();
    assert!(answer.contains("bank fd") && answer.contains("matures"), "{answer}");
}

#[tokio::test]
async fn an_investment_can_be_an_asset() {
    let s = Store::test().await.unwrap();
    let c = user(&s, "anita").await;
    let coins = s.add_asset(&c, gold(200_000_00)).await.unwrap();
    let before = s.insights(&c, InsightsQuery::default()).await.unwrap().assets;
    assert_eq!(before, 200_000_00);

    let acct = s
        .create_account(&c, NewAccount { asset_id: Some(coins.id), ..invest("digital gold", "gold", AccountDetails { invested: Some(120_000_00), ..Default::default() }) })
        .await
        .unwrap();
    // its value is the asset's, and the same coins are not counted twice
    assert_eq!(acct.balance, 200_000_00);
    assert_eq!(acct.asset.as_ref().map(|a| a.id), Some(coins.id));
    assert_eq!(s.insights(&c, InsightsQuery::default()).await.unwrap().assets, 200_000_00);
    assert_eq!(s.assets(&c).await.unwrap()[0].account.as_ref().map(|a| a.id), Some(acct.id));

    // it follows the asset
    s.update_asset(&c, coins.id, UpdateAsset { value: Some(250_000_00), ..Default::default() }).await.unwrap();
    assert_eq!(s.account(&c, acct.id).await.unwrap().balance, 250_000_00);
    assert_eq!(s.insights(&c, InsightsQuery::default()).await.unwrap().assets, 250_000_00);

    // so the value cannot be typed in here, and one asset backs one account
    assert!(s.update_account(&c, acct.id, UpdateAccount { balance: Some(1), ..Default::default() }).await.is_err());
    assert!(s.create_account(&c, NewAccount { asset_id: Some(coins.id), ..invest("again", "gold", AccountDetails::default()) }).await.is_err());

    // unlinking keeps the value it had, and the asset counts on its own again
    let free = s.update_account(&c, acct.id, UpdateAccount { asset_id: Some(None), ..Default::default() }).await.unwrap();
    assert!(free.asset.is_none());
    assert_eq!(free.balance, 250_000_00);
    assert_eq!(s.insights(&c, InsightsQuery::default()).await.unwrap().assets, 500_000_00);
}

#[tokio::test]
async fn linking_has_rules() {
    let s = Store::test().await.unwrap();
    let a = user(&s, "anita").await;
    let v = user(&s, "vikram").await;
    let mine = s.add_asset(&a, gold(100_00)).await.unwrap();
    // someone else's asset
    assert!(s.create_account(&v, NewAccount { asset_id: Some(mine.id), ..invest("x", "gold", AccountDetails::default()) }).await.is_err());
    // a market investment is not a thing you own outright
    assert!(s.create_account(&a, NewAccount { asset_id: Some(mine.id), ..invest("x", "mutual fund", AccountDetails::default()) }).await.is_err());
    // nor a bank account
    let bank = NewAccount { name: "b".into(), kind: AccountKind::Bank, ..invest("b", "", AccountDetails::default()) };
    assert!(s.create_account(&a, NewAccount { asset_id: Some(mine.id), ..bank }).await.is_err());
    // the type cannot be changed to a market one while linked
    let acct = s.create_account(&a, NewAccount { asset_id: Some(mine.id), ..invest("g", "gold", AccountDetails::default()) }).await.unwrap();
    let r = s.update_account(&a, acct.id, UpdateAccount { details: Some(AccountDetails { invest_kind: "stocks".into(), ..Default::default() }), ..Default::default() }).await;
    assert!(r.is_err());
}

#[tokio::test]
async fn deleting_the_asset_keeps_the_value() {
    let s = Store::test().await.unwrap();
    let c = user(&s, "anita").await;
    let flat = s.add_asset(&c, NewAsset { name: "flat".into(), kind: "property".into(), bought: String::new(), cost: None, value: 50_00_000_00, note: String::new() }).await.unwrap();
    let acct = s.create_account(&c, NewAccount { asset_id: Some(flat.id), ..invest("flat in pune", "real estate", AccountDetails::default()) }).await.unwrap();
    assert_eq!(acct.balance, 50_00_000_00);
    s.delete_asset(&c, flat.id).await.unwrap();
    let after = s.account(&c, acct.id).await.unwrap();
    assert!(after.asset.is_none());
    assert_eq!(after.balance, 50_00_000_00, "nothing jumps when the asset goes");
}

#[tokio::test]
async fn the_answer_to_how_are_my_investments_does_not_mix_up_costs() {
    let s = Store::test().await.unwrap();
    let c = user(&s, "anita").await;
    // one with a known cost, one without: the gain must only compare like with like
    s.create_account(&c, NewAccount { balance: Some(12_000_00), ..invest("index", "mutual fund", AccountDetails { invested: Some(10_000_00), ..Default::default() }) }).await.unwrap();
    s.create_account(&c, NewAccount { balance: Some(90_000_00), ..invest("old shares", "stocks", AccountDetails::default()) }).await.unwrap();
    let r = s.ask(&c, "how are my investments doing?").await.unwrap();
    assert!(r.contains("1,02,000") || r.contains("102,000") || r.contains("102000"), "total worth: {r}");
    assert!(r.contains("up") && !r.contains("up ₹92") && !r.contains("up 92"), "gain compares only the known: {r}");
}

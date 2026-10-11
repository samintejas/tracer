# pebblelab/fin

Part of [Pebble Lab](https://pebblelab.in), a collection of small, simple apps. This one lives at `fin.pebblelab.in`.

A household money tracker. Accounts (bank, credit card, loan, investment), transactions with tags, a family that can
share or co-own accounts, insights, and an assistant you can ask in plain words.

The backend is headless: one binary, `pebblelab`, is a **CLI**, a **REST API** and an **MCP server** over a Postgres
database (in Docker). The web app is a separate Leptos (client side) build served by the same binary, on the
[dots-ui](https://crates.io/crates/dots-ui) design system.

```
crates/api      wire types shared by everything (money, loan maths). No I/O. The UI uses it too.
crates/core     the rules, over Postgres (sqlx). The only code that touches the database.
compose.yml     the database: postgres 17, local port 5432, data in a named volume
crates/server   the `pebblelab` binary: CLI, REST (/api), MCP (/mcp and stdio)
ui/             Leptos web app (wasm, its own workspace)
```

## run

```sh
docker compose up -d                         # postgres on 127.0.0.1:5432 (user, password, db: pebblelab)
cargo run -p pebblelab -- user add "anita rao" anita@rao.example --password 'a long passphrase'
cargo run -p pebblelab -- serve                 # http://127.0.0.1:3000

# the web app (once)
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked   # must match ui/Cargo.lock
ui/build.sh                                  # builds ui/dist; PROFILE=debug for a quick build
```

`PEBBLELAB_DB` (default `postgres://pebblelab:pebblelab@localhost:5432/pebblelab`), `PEBBLELAB_LISTEN` and `PEBBLELAB_UI_DIR` set the
database, address and web app folder; `.env.example` lists them with the compose settings (`PEBBLELAB_PG_PASSWORD`,
`PEBBLELAB_PG_PORT`). The schema is created and migrated when the binary starts. `docker compose down -v` wipes the data.
`scripts/demo.sh` fills an empty database with a sample household to look around in.
A forgotten password is reset from an emailed link (Resend: set `PEBBLELAB_RESEND_API_KEY` and `PEBBLELAB_MAIL_FROM`). Set `PEBBLELAB_NO_PASSWORD_LOGIN=true` to let people in only through Google or GitHub: the email-and-password screens are hidden and their routes refuse. Without a key no email is sent, and the server's owner resets it with `pebblelab user passwd <email>`.

## running it for real

- **Put TLS in front.** The server speaks plain http on 127.0.0.1. Run it behind a reverse proxy (Caddy, nginx) that
  terminates https, and set `PEBBLELAB_TRUST_PROXY=true` so the rate limits see each client's address. Add
  `Strict-Transport-Security` at the proxy.
- **Close sign-ups** once the people who should be in are in: `PEBBLELAB_SIGNUPS=closed`. `pebblelab user add` still works.
- **Set `PEBBLELAB_TZ`** (for example `Asia/Kolkata`). "Today", subscription renewals and reminders follow it.
- **Sign in with Google and GitHub** show up on the sign-in and sign-up pages once their client id and secret are
  set (`PEBBLELAB_GOOGLE_*`, `PEBBLELAB_GITHUB_*`) along with `PEBBLELAB_PUBLIC_URL`. Register
  `<PEBBLELAB_PUBLIC_URL>/api/auth/google/callback` (and `.../github/callback`) as the redirect address with each.
  Google: create an OAuth client of type "web application", ask for no more than `openid email profile`, and switch
  the consent screen to "in production" (it needs a homepage and privacy-policy link). GitHub: a plain OAuth app.
  A provider must vouch for the email. If an account already has that address it is linked, and what was set up
  under the address before (sessions, password) is cut off, since a password sign-up never proved the address was
  theirs. They can set a new password under profile. With `PEBBLELAB_SIGNUPS=closed` an unknown address is refused.
- **Background work** (subscription renewals, reminders, removing expired sessions and old notifications) runs inside
  `serve` every `PEBBLELAB_JOB_SECS`. Reads never write. With `PEBBLELAB_JOB_SECS=0`, run `pebblelab jobs` from cron instead.
- **Health check:** `GET /health` answers 200 only while the database does.
- **Back up the database** (`pg_dump`), and change the compose password: it ships as `pebblelab`.
- Sessions end after 30 days unused (180 at most). Sign-in is limited to 8 tries per address per 15 minutes, and a
  response to too many tries is `429`.

## cli

It works on the database directly (no server needed). `--as <email>` (or `PEBBLELAB_USER`) says whose data it is;
`PEBBLELAB_TOKEN` acts as a scoped API token instead. `--json` prints JSON.

```sh
pebblelab account add salary --balance 184250
pebblelab account add "home loan" --kind loan --total 2500000 --rate 8.5 --tenure 180 --start 2019-04 --emi-day 5
pebblelab tx add salary 3240 groceries, weekly --tag groceries,household
pebblelab tx transfer salary "card, anita" 20000
pebblelab tx list --tag groceries
pebblelab sub add "streaming video" 649 "card, anita" --tag entertainment --next 2026-11-12   # renewals become transactions
pebblelab sub list
pebblelab asset add "apartment, pune" 8200000 --kind property --bought 2019-04 --cost 6500000
pebblelab asset value 1 8400000                # what it is worth now
pebblelab summary
pebblelab ask "when do my loans end?"
pebblelab family create "rao family"
pebblelab family invite                     # a one-time code; the other person runs `family join <code>`
pebblelab token create claude --scopes read,transactions,add
```

## rest

`/api/*`, JSON, `Authorization: Bearer <token>`. Amounts are decimal strings (`"3240.50"`) in major units. Sign in
at `POST /api/auth/signin` for a session token, or create a connector token (profile, connectors).
Routes: `auth/{signup,signin,signout,signout-all,reset,reset/confirm,providers,redeem}`, `auth/{google,github}/{start,callback}`, `me` (also delete), `me/password`, `export.csv`,
`family` (create, delete), `family/{invite,join,leave}`, `accounts` (and `accounts/{id}/leave`), `transactions` (filter, sort, page, in/out
totals), `transfers`, `transactions/{id}/attachments`, `subscriptions`, `assets`, `tags`, `insights`, `ask`, `notifications`, `connectors`.

## mcp

`POST /mcp` (streamable HTTP, bearer token) or `pebblelab mcp` on stdio (`PEBBLELAB_TOKEN`, and `PEBBLELAB_DB` if not the default).
Tools: `list_accounts`, `create_account`, `update_account`, `list_transactions`, `add_transaction`,
`transfer_money`, `update_transaction`, `delete_transaction`, `list_subscriptions`, `add_subscription`, `update_subscription`, `delete_subscription`, `list_assets`, `add_asset`,
`update_asset`, `delete_asset`, `list_tags`, `get_insights`, `ask_pebblelab`.
A token has scopes: `read`, `transactions` (read them), `add`, `edit`; a tool outside them reports the missing scope.

## model

- Amounts are integer minor units; balances are never stored (`opening + sum(transactions)`); a loan's balance
  comes from its schedule.
- An account is private (owners only) or shared (the owner's family can see it). A bank account can be joint:
  owned by several family members, who all see it and add to it.
  Only owners change an account or add to it. A co-owner can be added but only takes themselves off
  (`accounts/{id}/leave`); an account with several owners can only be deleted once the others have left. A transfer
  is between accounts you own.
- A transfer is two linked transactions, edited and deleted together.
- A subscription is a monthly or yearly charge on an account you own. When its renewal date arrives a debit is
  added (tagged with its category and `subscription`) and the date moves on. This happens when anything that
  depends on balances is read, once per renewal, and a paused subscription adds nothing. Subscriptions are yours
  alone.
- An asset is something you own outside any account (property, vehicle, gold, electronics): the price paid and
  what it is worth now, which you keep up to date. It counts towards assets and net worth in insights, in your
  own and the family view, never in another member's. Assets are yours alone.
- An investment has a type, and the type decides what it asks for. Market (mutual fund, stocks, etf, bonds,
  crypto): invested, current value, optional sip. Deposits (fixed, recurring): rate, opening month and term, from
  which its worth now and at maturity is estimated (quarterly compounding; the bank's figure wins if you type
  one). Retirement (ppf, epf, nps): statement value, monthly contribution, optional rate. Physical (gold, real
  estate) and other: price paid and value. The physical and other types can be linked to one of your assets:
  the account's balance is then the asset's value, and net worth counts it once. Unlinking, or deleting the
  asset, keeps the value the account had.

## test

```sh
cargo test                       # needs the database up. each test gets its own schema. core: auth, family visibility, balances, transfers, loans, insights, ask, one test per past bug (tests/hardening.rs); server: REST permissions, scopes, rate limits, headers
```

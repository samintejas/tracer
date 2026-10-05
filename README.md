# tracer/fin

A household money tracker. Accounts (bank, credit card, loan, investment), transactions with tags, a family that can
share or co-own accounts, insights, and an assistant you can ask in plain words.

The backend is headless: one binary, `tracer`, is a **CLI**, a **REST API** and an **MCP server** over one SQLite
file. The web app is a separate Leptos (client side) build served by the same binary, on the
[dots](../dots-design) design system.

```
crates/api      wire types shared by everything (money, loan maths). No I/O. The UI uses it too.
crates/core     the rules, over SQLite (sqlx). The only code that touches the database.
crates/server   the `tracer` binary: CLI, REST (/api), MCP (/mcp and stdio)
ui/             Leptos web app (wasm, its own workspace)
```

## run

```sh
cargo run -p tracer -- user add "anita rao" anita@rao.example --password 'a long passphrase'
cargo run -p tracer -- serve                 # http://127.0.0.1:3000  (db: sqlite://tracer.db)

# the web app (once)
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked   # must match ui/Cargo.lock
ui/build.sh                                  # builds ui/dist; PROFILE=debug for a quick build
```

`TRACER_DB`, `TRACER_LISTEN`, `TRACER_UI_DIR` set the database, address and web app folder.
There is no email service, so a forgotten password is reset on the server: `tracer user passwd <email>`.

## cli

It works on the database directly (no server needed). `--as <email>` (or `TRACER_USER`) says whose data it is;
`TRACER_TOKEN` acts as a scoped API token instead. `--json` prints JSON.

```sh
tracer account add salary --balance 184250
tracer account add "home loan" --kind loan --total 2500000 --rate 8.5 --tenure 180 --start 2019-04 --emi-day 5
tracer tx add salary 3240 groceries, weekly --tag groceries,household
tracer tx transfer salary "card, anita" 20000
tracer tx list --tag groceries
tracer summary
tracer ask "when do my loans end?"
tracer family create "rao family"
tracer family invite                     # a one-time code; the other person runs `family join <code>`
tracer token create claude --scopes read,transactions,add
```

## rest

`/api/*`, JSON, `Authorization: Bearer <token>`. Amounts are decimal strings (`"3240.50"`) in major units. Sign in
at `POST /api/auth/signin` for a session token, or create a connector token (profile, connectors).
Routes: `auth/{signup,signin,signout,reset}`, `me`, `me/password`, `family`, `family/join`, `accounts`,
`transactions` (filter, sort, page), `transfers`, `transactions/{id}/attachments`, `tags`, `insights`, `ask`,
`notifications`, `connectors`.

## mcp

`POST /mcp` (streamable HTTP, bearer token) or `tracer mcp` on stdio (`TRACER_TOKEN`, `TRACER_DB`).
Tools: `list_accounts`, `create_account`, `update_account`, `list_transactions`, `add_transaction`,
`transfer_money`, `update_transaction`, `delete_transaction`, `list_tags`, `get_insights`, `ask_tracer`.
A token has scopes: `read`, `transactions` (read them), `add`, `edit`; a tool outside them reports the missing scope.

## model

- Amounts are integer minor units; balances are never stored (`opening + sum(transactions)`); a loan's balance
  comes from its schedule.
- An account is private (owners only) or shared (the owner's family can see it); more than one owner makes it joint.
  Only owners change an account or add to it.
- A transfer is two linked transactions, edited and deleted together.

## test

```sh
cargo test                       # core: auth, family visibility, balances, transfers, loans, insights, ask
```

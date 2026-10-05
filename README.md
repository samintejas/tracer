# tracer/fin

A household money tracker. Accounts (bank, credit card, loan, investment), transactions with tags, a family that can
share or co-own accounts, insights, and an assistant you can ask in plain words.

The backend is headless: one binary, `tracer`, is a **CLI**, a **REST API** and an **MCP server** over a Postgres
database (in Docker). The web app is a separate Leptos (client side) build served by the same binary, on the
[dots](../dots-design) design system.

```
crates/api      wire types shared by everything (money, loan maths). No I/O. The UI uses it too.
crates/core     the rules, over Postgres (sqlx). The only code that touches the database.
compose.yml     the database: postgres 17, local port 5432, data in a named volume
crates/server   the `tracer` binary: CLI, REST (/api), MCP (/mcp and stdio)
ui/             Leptos web app (wasm, its own workspace)
```

## run

```sh
docker compose up -d                         # postgres on 127.0.0.1:5432 (user, password, db: tracer)
cargo run -p tracer -- user add "anita rao" anita@rao.example --password 'a long passphrase'
cargo run -p tracer -- serve                 # http://127.0.0.1:3000

# the web app (once)
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked   # must match ui/Cargo.lock
ui/build.sh                                  # builds ui/dist; PROFILE=debug for a quick build
```

`TRACER_DB` (default `postgres://tracer:tracer@localhost:5432/tracer`), `TRACER_LISTEN` and `TRACER_UI_DIR` set the
database, address and web app folder; `.env.example` lists them with the compose settings (`TRACER_PG_PASSWORD`,
`TRACER_PG_PORT`). The schema is created and migrated when the binary starts. `docker compose down -v` wipes the data.
`scripts/demo.sh` fills an empty database with a sample household to look around in.
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
Routes: `auth/{signup,signin,signout,signout-all,reset}`, `me` (also delete), `me/password`, `export.csv`,
`family` (create, delete), `family/{invite,join,leave}`, `accounts`, `transactions` (filter, sort, page, in/out
totals), `transfers`, `transactions/{id}/attachments`, `tags`, `insights`, `ask`, `notifications`, `connectors`.

## mcp

`POST /mcp` (streamable HTTP, bearer token) or `tracer mcp` on stdio (`TRACER_TOKEN`, and `TRACER_DB` if not the default).
Tools: `list_accounts`, `create_account`, `update_account`, `list_transactions`, `add_transaction`,
`transfer_money`, `update_transaction`, `delete_transaction`, `list_tags`, `get_insights`, `ask_tracer`.
A token has scopes: `read`, `transactions` (read them), `add`, `edit`; a tool outside them reports the missing scope.

## model

- Amounts are integer minor units; balances are never stored (`opening + sum(transactions)`); a loan's balance
  comes from its schedule.
- An account is private (owners only) or shared (the owner's family can see it). A bank account can be joint:
  owned by several family members, who all see it and add to it.
  Only owners change an account or add to it.
- A transfer is two linked transactions, edited and deleted together.

## test

```sh
cargo test                       # needs the database up. each test gets its own schema. core: auth, family visibility, balances, transfers, loans, insights, ask
```

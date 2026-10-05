#!/usr/bin/env bash
# Fill the database with the sample household from the design: anita and vikram rao, a family, twelve
# accounts and six months of transactions. For trying the app; not for real data.
#   scripts/demo.sh [postgres://…]        default: the docker compose database. password for both: "correct horse battery"
# It adds to what is there, so run it on an empty database (docker compose down -v && docker compose up -d).
set -euo pipefail
cd "$(dirname "$0")/.."
export TRACER_DB="${1:-${TRACER_DB:-postgres://tracer:tracer@localhost:5432/tracer}}" TRACER_PASSWORD='correct horse battery'
T="${TRACER_BIN:-target/debug/tracer}"
[ -x "$T" ] || cargo build -q -p tracer
A=anita@rao.example; V=vikram@rao.example
a() { TRACER_USER=$A "$T" "$@" >/dev/null; }
v() { TRACER_USER=$V "$T" "$@" >/dev/null; }
d() { date -d "$1" +%F; }                      # d "3 days ago"

"$T" user add "anita rao" $A >/dev/null; "$T" user add "vikram rao" $V >/dev/null
a family create "rao family"
CODE=$(TRACER_USER=$A "$T" family invite | sed -n 's/.*: \([A-Z0-9-]\{9\}\)$/\1/p')
v family join "$CODE"

a account add "salary account" --balance 184250 --institution hdfc
v account add "savings" --balance 96400 --shared
a account add "emergency fund" --balance 450000 --shared
a account add "household joint" --balance 212800 --with $V
a account add "card, anita" --kind credit --balance 38420 --limit 300000 --due-day 18
v account add "card, vikram" --kind credit --balance 12150 --limit 150000 --due-day 22 --shared
a account add "home loan" --kind loan --total 2500000 --rate 8.5 --tenure 180 --start 2019-04 --emi-day 5 --shared
v account add "car loan" --kind loan --total 800000 --rate 9.5 --tenure 60 --start 2023-10 --emi-day 28 --shared
a account add "mutual funds, sip" --kind investment --balance 685300 --invested 600000 --shared
v account add "index fund" --kind investment --balance 320000 --invested 290000 --shared
a account add "ppf" --kind investment --balance 410000 --invested 365000
a account add "fixed deposit" --kind investment --balance 207400 --invested 200000 --shared

# five earlier months: salaries in, the usual out
for m in 5 4 3 2 1; do
  a tx add "salary account" $((138000 + (5 - m) * 1000)) salary --in --tag income --date "$(d "$m months ago")"
  v tx add savings $((112000 + (5 - m) * 1500)) salary --in --tag income --date "$(d "$m months ago")"
  a tx add "household joint" $((2600 + m * 90)) groceries, weekly --tag groceries,household --date "$(d "$m months ago + 3 days")"
  v tx add "household joint" $((2700 + m * 60)) groceries, weekly --tag groceries,household --date "$(d "$m months ago + 11 days")"
  v tx add "household joint" $((2500 + m * 120)) electricity bill --tag utilities --date "$(d "$m months ago + 6 days")"
  a tx add "household joint" 18500 school fees --tag education --date "$(d "$m months ago + 1 day")"
  v tx add "card, vikram" $((1800 + m * 150)) fuel --tag transport --date "$(d "$m months ago + 9 days")"
  a tx add "card, anita" $((1500 + m * 210)) dinner out --tag dining --date "$(d "$m months ago + 14 days")"
  a tx transfer "salary account" "household joint" 45000 --date "$(d "$m months ago")"
  v tx transfer savings "household joint" 40000 --date "$(d "$m months ago")"
  v tx transfer savings "car loan" 16450 --date "$(d "$m months ago + 2 days")"
  v tx transfer "household joint" "home loan" 24618 --date "$(d "$m months ago + 4 days")"
done
# the last few weeks, day by day
a tx add "salary account" 142000 salary --in --tag income --date "$(d "4 days ago")"
v tx add savings 118000 salary --in --tag income --date "$(d "4 days ago")"
a tx add "household joint" 18500 school fees --tag education --date "$(d "4 days ago")"
a tx transfer "salary account" "household joint" 45000 --date "$(d "4 days ago")"
v tx transfer savings "household joint" 40000 --date "$(d "4 days ago")"
v tx transfer "household joint" "home loan" 24618
a tx add "household joint" 3240 groceries, weekly --tag groceries,household --date "$(d "1 day ago")"
v tx add "card, vikram" 2100 fuel --tag transport --date "$(d "2 days ago")"
v tx transfer savings "index fund" 10000 --date "$(d "3 days ago")"
v tx add "household joint" 2860 electricity bill --tag utilities --date "$(d "5 days ago")"
a tx add "card, anita" 1980 dinner out --tag dining --date "$(d "6 days ago")"
v tx transfer savings "car loan" 16450 --date "$(d "7 days ago")"
a tx transfer "salary account" "mutual funds, sip" 15000 --date "$(d "8 days ago")"
a tx add "card, anita" 640 pharmacy --tag health --date "$(d "9 days ago")"
a tx add "household joint" 999 internet --tag utilities --date "$(d "10 days ago")"
v tx add "household joint" 2870 groceries, weekly --tag groceries,household --date "$(d "11 days ago")"
v tx add "card, vikram" 1450 train tickets --tag transport --date "$(d "13 days ago")"
a tx transfer "salary account" "emergency fund" 20000 --date "$(d "15 days ago")"
echo "demo data in $TRACER_DB. sign in as $A or $V, password: $TRACER_PASSWORD"

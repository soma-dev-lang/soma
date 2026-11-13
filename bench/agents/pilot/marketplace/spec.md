# Marketplace — what to build

A two-sided market: buyers pay into escrow, sellers ship, the money moves
when the goods arrive — or goes back when a dispute says so.

Build it as six cells with exactly these names and public signals (names,
parameter order and types are fixed: other code calls them). What each cell
stores, its state machines and its private helpers are yours to design.
All amounts are integers (cents). No HTTP server is needed beyond `Api.request`.

Errors are raised with `require … else Kind "message"` (or `fail("Kind", "message")`);
the kinds to use are named below. A refusal that is not an error returns a
value, as stated.

## Catalog — what is for sale

- `list_item(listing: String, seller: String, price: Int, units: Int) -> Map`
  Creates a listing. `price >= 1`, `units >= 0` (otherwise an error). A listing
  id that already exists: error kind `Taken`. Returns a map with the fields
  `listing`, `seller`, `price`, `stock` (the units).
- `listing(listing: String) -> Map` — the same four fields, `stock` being the
  units left now. Unknown listing: error kind `NotFound`.
- `seller_of(listing: String) -> String` — the seller, `""` if unknown.
- `price_of(listing: String) -> Int` — the unit price, `0` if unknown.
- `reserve(listing: String, units: Int) -> Bool` — takes `units` (>= 1) off the
  shelf if that many are left and returns `true`; otherwise returns `false`
  and changes nothing.
- `restock(listing: String, units: Int) -> Int` — puts `units` (>= 1) back;
  returns the units left.

Stock never goes below zero.

## Wallets — the only cell that holds money

- `deposit(owner: String, amount: Int) -> Int` — `amount >= 1`; returns the new balance.
- `balance(owner: String) -> Int` — `0` for an unknown owner.
- `debit(owner: String, amount: Int) -> Bool` — `amount >= 1`; takes the money if
  the owner has enough and returns `true`; otherwise `false`, nothing changes.
- `credit(owner: String, amount: Int) -> Int` — `amount >= 1`; returns the new balance.

A balance never goes below zero. No other cell stores or changes balances.

## Escrow — money in flight

A hold has a lifecycle: it does not exist (`"none"`), then it is `"held"`,
then it ends exactly once, either `"released"` (to the payee) or `"refunded"`
(back to the payer). Both ends are final.

- `open_hold(id: String, payer: String, payee: String, amount: Int) -> Bool` —
  `amount >= 1`. Debits the payer; if the payer cannot pay returns `false` and
  no hold exists. Otherwise the hold is `held` and the answer is `true`.
- `release(id: String) -> Int` — ends a held hold in favour of the payee: credits
  the payee the held amount, returns the payee's new balance. A hold that is not
  `held` cannot be released (an error; nothing changes).
- `refund(id: String) -> Int` — same, towards the payer; returns the payer's new balance.
- `hold_state(id: String) -> String` — `"none"`, `"held"`, `"released"` or `"refunded"`.
- `held_amount(id: String) -> Int` — the amount while the hold is `held`, else `0`.

Only Escrow moves money between wallets.

## Orders — the lifecycle of a purchase

States: `draft` (not placed yet) → `paid` → `shipped` → `delivered`; a `paid`
order can be `cancelled`; a `shipped` order can be `disputed`, and a disputed
order ends `delivered` (ruling for the seller) or `returned` (ruling for the
buyer). `delivered`, `cancelled` and `returned` are final. Any move the
lifecycle does not allow is an error and changes nothing.

- `place(id: String, buyer: String, listing: String, n: Int) -> Map` — `n >= 1`.
  An order id that already exists: error kind `Taken`. Unknown listing: error
  kind `NotFound`. Reserves `n` units, then opens an escrow hold **with the
  order's id** from the buyer to the listing's seller for `price × n`.
  - not enough stock: returns a map with `ok: false`, `reason: "out of stock"`; nothing changes;
  - the buyer cannot pay: the units go back on the shelf; returns `ok: false`,
    `reason: "insufficient funds"`; no order exists;
  - otherwise the order is `paid`; returns `ok: true`, `order` (the id), `total`, `state: "paid"`.
- `ship(id: String, seller: String) -> Map` — only the listing's seller (else error
  kind `Forbidden`); unknown order: `NotFound`. `paid` → `shipped`. Returns
  `ok: true`, `order`, `state: "shipped"`.
- `confirm_delivery(id: String, buyer: String) -> Map` — only the order's buyer
  (else `Forbidden`). `shipped` → `delivered`, and the hold is released to the
  seller. Returns `ok: true`, `order`, `state: "delivered"`, `seller_balance`.
- `cancel(id: String, buyer: String) -> Map` — only the buyer (else `Forbidden`).
  `paid` → `cancelled`: the units go back on the shelf and the hold is refunded.
  Returns `ok: true`, `order`, `state: "cancelled"`, `buyer_balance`.
- `open_dispute(id: String, buyer: String) -> Map` — only the buyer (else
  `Forbidden`). `shipped` → `disputed`. Returns `ok: true`, `order`, `state: "disputed"`.
- `close_dispute(id: String, for_buyer: Bool) -> String` — `disputed` → `returned`
  (and the units go back on the shelf) when `for_buyer`, else → `delivered`.
  Returns the new state. It does not move money: `Disputes.resolve` does.
- `order_state(id: String) -> String` — the state; `"draft"` for an unknown id.
- `order(id: String) -> Map` — fields `order`, `buyer`, `listing`, `units`, `state`.
  Unknown order: `NotFound`.

## Disputes — who decides when buyer and seller disagree

- `file(order_id: String, buyer: String, reason: String) -> Map` — opens the
  dispute on the order (`Orders.open_dispute`, whose answer it returns) and
  records the reason.
- `resolve(order_id: String, ruling: String) -> Map` — `ruling` is `"buyer"` or
  `"seller"` (anything else: error kind `BadRuling`); no dispute filed on that
  order: `NotFound`. Records the ruling, closes the dispute on the order, and
  moves the money: refund for `"buyer"`, release for `"seller"`. All of it or
  none of it. Returns `order`, `ruling`, `state` (the order's new state) and
  `buyer_balance` (ruling for the buyer) or `seller_balance` (for the seller).
- `ruling(order_id: String) -> String` — `"buyer"`, `"seller"`, or `"none"`.

## Api — the HTTP surface

- `request(method: String, path: String, body: String) -> Map` — `body` is JSON
  (or `""`). Routes:

  | Route | Does | Answers |
  |---|---|---|
  | `POST /listings` `{listing, seller, price, units}` | `list_item` | its answer |
  | `GET /listings/<id>` | `listing` | its answer |
  | `POST /wallets/<owner>` `{amount}` | `deposit` | `{owner, balance}` |
  | `GET /wallets/<owner>` | `balance` | `{owner, balance}` |
  | `POST /orders` `{order, buyer, listing, units}` | `place` | its answer |
  | `GET /orders/<id>` | `order` + the hold | `{order, buyer, listing, units, state, hold}` (`hold`: the hold's state) |
  | `POST /ship/<id>` `{seller}` | `ship` | its answer |
  | `POST /confirm/<id>` `{buyer}` | `confirm_delivery` | its answer |
  | `POST /cancel/<id>` `{buyer}` | `cancel` | its answer |
  | `POST /dispute/<id>` `{buyer, reason}` | `file` | its answer |
  | `POST /ruling/<id>` `{ruling}` | `resolve` | its answer |

  An error raised by a route becomes `response(status, map("error", kind, "detail", …))`:
  `NotFound` → 404, `Forbidden` → 403, `Taken` → 409, any other → 400. An unknown
  route answers 404. (`response(status, map)` is the builtin; the status is
  then readable as `._status`.)

Api stores nothing and changes nothing itself.

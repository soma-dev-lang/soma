# Mesa — food delivery in Soma

A full platform in one `.cell` file: restaurants, orders, payments,
couriers, analytics, and a dark-mode dashboard — with the order
lifecycle **proven** by the verifier.

```
soma verify delivery/app.cell    # eventually(Delivered | Cancelled): PROVEN
soma test   delivery/app.cell    # 11 declarative assertions
soma serve  delivery/app.cell -p 8080
open http://localhost:8080
```

## Architecture

| Cell | Role |
|------|------|
| `Mesa` | HTTP edge — owns `request`, every route is a thin wrapper |
| `Orders` | Typed state machine `Placed → Accepted → Cooking → Ready → PickedUp → Delivered` (+ `Cancelled`) |
| `Payments` | Sum-type results: `Charged{tx,amount} \| Declined{reason} \| CashOnDelivery` |
| `Menu` | Seeded catalog, persistent |
| `Couriers` | Deliberately cyclic machine `idle → assigned → delivering → idle` |
| `Analytics` | `filter_by / sum_by / agg` pipelines over the order board |

## What the verifier proves

- `eventually(state in [Delivered, Cancelled])` — no order is ever stuck
- `after(PickedUp, Delivered)` — picked-up food always arrives
- the courier machine is recognized as a reactive system (warning, not failure)
- properties are scoped via `[verify] cells = ["Orders"]` in `soma.toml`

## Domain rules enforced by the machine, not by `if`s

- can't deliver before pickup (409 with counter-example)
- can't cancel after pickup
- a courier on a delivery can't be double-booked (`is_available` gate)
- a declined payment means the order never exists

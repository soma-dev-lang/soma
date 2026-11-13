# Hotel — what to build

A small hotel: rooms with nightly rates, a calendar of nights, the lifecycle
of a stay, a folio per booking, housekeeping, and loyalty points.

Build it as seven cells with exactly these names and public signals (names,
parameter order and types are fixed: other code calls them). What each cell
stores, its state machines and its private helpers are yours to design.
Amounts are integers (cents). A **day** is an integer; a stay `from … to`
covers the nights `from <= d < to` (it leaves on day `to`). There is no clock:
the day is always a parameter.

Errors are raised with `require … else Kind "message"` (or `fail("Kind", "message")`);
the kinds to use are named below. A refusal that is not an error returns a
value, as stated. An error changes nothing.

## Rooms — the registry

- `add_room(room: String, kind: String, rate: Int) -> Map` — `rate >= 1`. Existing
  room: error `Taken`. Returns a map with `room`, `kind`, `rate`.
- `room(room: String) -> Map` — the same three fields; unknown room: `NotFound`.
- `exists(room: String) -> Bool`
- `rate_of(room: String) -> Int` — the nightly rate; `0` for an unknown room.
- `set_rate(room: String, rate: Int) -> Int` — `rate >= 1`; unknown room: `NotFound`.
  Returns the new rate. Changing a rate never changes what an existing booking costs.
- `rooms_of_kind(kind: String) -> List` — the rooms of that kind, in the order
  they were added.

## Housekeeping — the state of each room right now

A room is `clean` (also a room never seen), `occupied`, `dirty`, `cleaning`
or `out_of_order`. The only moves: clean → occupied → dirty → cleaning →
clean, and clean → out_of_order → clean. Any other move is an error.

- `room_status(room: String) -> String`
- `occupy(room: String) -> String` — clean → occupied; returns `"occupied"`.
- `vacate(room: String) -> String` — occupied → dirty; returns `"dirty"`.
- `start_cleaning(room: String, staff: String) -> String` — dirty → cleaning,
  remembering who cleans; returns `"cleaning"`.
- `finish_cleaning(room: String, staff: String) -> String` — cleaning → clean,
  only by the staff who started it (else error `Forbidden`); returns `"clean"`.
- `block(room: String) -> String` — clean → out_of_order; returns `"out_of_order"`.
- `unblock(room: String) -> String` — out_of_order → clean; returns `"clean"`.

## Calendar — who holds which night

- `is_free(room: String, from: Int, to: Int) -> Bool` — `from < to` (else an
  error); true when none of the nights is held.
- `hold_nights(booking: String, room: String, from: Int, to: Int) -> Bool` —
  holds all the nights for that booking and returns `true`, or returns `false`
  and holds none if any is already held.
- `free_nights(booking: String, room: String, from: Int, to: Int) -> Int` —
  frees the nights of that range that this booking holds (and only those);
  returns how many.
- `held_by(room: String, day: Int) -> String` — the booking holding that night, `""` if none.

## Ledger — one folio per booking

A folio has the booking's id. It records what was charged, what was paid in
cash and what was paid with points; what is due is the difference.

- `charge(folio: String, amount: Int) -> Int` — `amount >= 1`; returns the due amount afterwards.
- `pay_cash(folio: String, amount: Int) -> Int` — `amount >= 1`; more than is due:
  error `Overpaid`. Returns the due amount afterwards.
- `pay_points(folio: String, amount: Int) -> Int` — the same, recorded as paid with points.
- `due(folio: String) -> Int` — `0` for an unknown folio.
- `cash_paid(folio: String) -> Int`
- `statement(folio: String) -> Map` — fields `charged`, `cash`, `points`, `due`.

## Loyalty — points per guest

One point is worth one cent when redeemed.

- `earn(guest: String, cash_amount: Int) -> Int` — credits one point per full 100
  cents; returns the guest's points afterwards.
- `redeem(guest: String, points: Int) -> Bool` — `points >= 1`; takes them and
  returns `true`, or returns `false` and takes nothing if the guest has fewer.
- `points_of(guest: String) -> Int`

## Bookings — the lifecycle of a stay

States: `requested` (not booked yet; also the answer for an unknown id) →
`confirmed` → `checked_in` → `checked_out`; a confirmed booking can instead
become `cancelled` or `no_show`. The last three are final. A move the
lifecycle does not allow is an error.

- `book(id: String, guest: String, room: String, from: Int, to: Int) -> Map` —
  `from < to`. Existing booking id: error `Taken`; unknown room: `NotFound`.
  If the nights are not all free: returns `ok: false`, `reason: "not available"`,
  and nothing is booked. Otherwise the nights are held under this booking's id,
  the booking is `confirmed`, and it keeps **the room's rate at this moment** as
  its nightly rate. Returns `ok: true`, `booking` (the id), `nights`, `total`
  (rate × nights).
- `cancel(id: String, guest: String, day: Int) -> Map` — only the booking's guest
  (else `Forbidden`); unknown id: `NotFound`. confirmed → cancelled and all its
  nights are freed. Cancelling two days or more before arrival (`day <= from - 2`)
  is free; later it costs one night at the booking's rate, charged to its folio.
  Returns `ok: true`, `fee`.
- `check_in(id: String, day: Int) -> Map` — `day` must be the arrival day (else
  error `WrongDay`) and the room must be `clean` (else error `NotReady`).
  confirmed → checked_in, the room becomes occupied, and the whole stay
  (booking's rate × nights) is charged to the folio. Returns `ok: true`, `due`.
- `check_out(id: String, day: Int) -> Map` — nothing may be due on the folio (else
  error `Unpaid`). checked_in → checked_out, the room becomes dirty, the nights
  from `day` on are freed (leaving early releases them; there is no refund), and
  the guest earns points on the **cash** paid on the folio. Returns `ok: true`,
  `points` (the guest's points afterwards).
- `mark_no_show(id: String) -> Int` — confirmed → no_show, all its nights are
  freed, one night at the booking's rate is charged. Returns that fee.
- `stay_state(id: String) -> String`
- `booking(id: String) -> Map` — fields `booking`, `guest`, `room`, `from`, `to`,
  `nightly`, `state`; unknown id: `NotFound`.
- `guest_of(id: String) -> String` — `""` if unknown.
- `arriving(day: Int) -> List` — the ids of the bookings that are `confirmed` and
  arrive on `day`, in the order they were booked.

## Desk — what the front desk does

Desk stores nothing.

- `quote(room: String, from: Int, to: Int) -> Int` — today's rate × nights;
  unknown room: `NotFound`.
- `settle(id: String, amount: Int) -> Map` — a cash payment on the booking's
  folio. Returns `ok: true`, `due`.
- `settle_with_points(id: String, points: Int) -> Map` — pays `points` cents of
  the folio with the booking guest's points. More than is due: error `Overpaid`,
  and no points are taken. The guest has too few: returns `ok: false`,
  `reason: "not enough points"`, nothing changes. Otherwise `ok: true`, `due`.
- `night_audit(day: Int) -> Map` — every booking still `confirmed` that was to
  arrive on `day` becomes a no-show. Returns `no_shows` (how many) and `fees`
  (the sum of their fees).
- `move_room(id: String, new_id: String, room: String) -> Map` — moves a confirmed
  booking to another room for the same nights. Unknown new room: `NotFound`. The
  new room is not free on all the nights: returns `ok: false`,
  `reason: "not available"`, nothing changes. Otherwise the old booking is
  cancelled without fee and a new booking `new_id` is made for the same guest at
  the new room's current rate; returns that booking's answer (as `book`).

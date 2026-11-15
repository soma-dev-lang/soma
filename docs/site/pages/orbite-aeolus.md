---
title: Aeolus and Starlink 44, replayed in Orbite
full_title: Aeolus vs Starlink 44 (2019), replayed in Orbite — what a shared protocol would have done, line by line
description: In September 2019 ESA's Aeolus manoeuvred alone to avoid Starlink 44 after its messages went unanswered. Orbite, a space traffic coordination system written in Soma, replays the incident on its real code: alert, escalation of the silent operator, take-over, a single designation, a verified manoeuvre — and what Orbite would have done worse.
type: TechArticle
toc: yes
---

# Aeolus and Starlink 44, replayed in Orbite

<p class="lede">On 2 September 2019, ESA's Earth-observation satellite Aeolus fired its thrusters to avoid Starlink 44. The risk of collision had climbed to about 1 in 1,000, ten times ESA's threshold for acting, and ESA could not get an answer from SpaceX. There was no rule saying who should move. We replayed the incident in <a href="/orbite">Orbite</a>, a simulated coordination protocol written in Soma — on its real code, not on a model — to see what a shared protocol would have done, line by line. It does several things right, and one thing worse than ESA did.</p>

## What happened

As reported by both parties at the time:

- **28 August 2019** — a conjunction alert between Aeolus and Starlink 44, with a probability of collision of about 1 in 50,000, below the threshold for a manoeuvre. In an e-mail exchange SpaceX said it did not plan to move at that level of risk.
- **The following days** — the probability rose to about 1 in 1,000, ten times ESA's threshold (1 in 10,000). ESA sought to coordinate and got no answer. SpaceX later explained that a bug in its on-call paging system kept its operator from seeing the follow-up messages, and that it would otherwise have coordinated.
- **2 September 2019** — Aeolus raised its orbit, about half an orbit before the closest approach, and passed above Starlink 44.
- No international rule said which of the two should manoeuvre.

The accounts differ on how hard each side tried to reach the other once the threshold was crossed. The replay does not settle that; it asks what happens when one side is silent, for whatever reason.

## What Orbite would have done

The replay is a test, [`test_aeolus.cell`](/cases/orbite/src/test_aeolus.cell): 95 assertions on Orbite's real bodies, no test doubles, driven by Orbite's own clock. Only the four facts above are historical. Everything else — Orbite itself (nothing like it existed in 2019), its fictional States, people, rules of the road and thresholds, the exact hours (alert on 28 August at 10:02, rise on 30 August at 14:02, closest approach on 2 September at 12:00 UTC), distances and Δv — is simulated. Aeolus has impulsive propulsion; Starlink has low-thrust electric propulsion, which needs days of notice to manoeuvre.

| When (UTC) | What Orbite does |
|---|---|
| 28 Aug, 10:02 | Conjunction opened at 1 in 50,000: level 1. No coordination, no designation. |
| 30 Aug, 14:02 | Probability up to 1 in 1,000: level 3. Coordination opened. The rules of the road propose **Aeolus**, as the only one able to act: Starlink's electric propulsion needs 72 h of notice, and the closest approach is 70 h away. |
| 30 Aug, 14:10 | ESA accepts. |
| 30 Aug, 15:00 | SpaceX has not answered: a breach is recorded (late answer, severity 3) and escalation starts — SpaceX's on-call duty. |
| 15:30 · 16:00 · 31 Aug, 01:00 | Escalation climbs: mission lead, then the focal point of SpaceX's State, then Orbite's direction and the focal points of both States. |
| 2 Sep, 04:00 | **Orbite takes over**, 8 h before the closest approach (Aeolus' 6-hour notice plus 2 h to arbitrate). |
| 2 Sep, 04:05 | Arbitration with four eyes (an analyst and a supervisor): Aeolus manoeuvres. One lock, one decision. |
| 2 Sep, 04:20 | Aeolus' avoidance plan declared, its trajectory screened against the catalogue (no new conjunction), approved. |
| 2 Sep, 11:15 | Burn, 45 minutes — about half an orbit — before the closest approach. |
| 2 Sep, 11:50 | Orbit observed and found conforming; the conjunction is resolved, the coordination closed, the lock retired. |

The full trace, one line per event of Orbite's journal, is in [AEOLUS_TRACE.md](/cases/orbite/src/AEOLUS_TRACE.md) (in French, like the code).

## What it got right

- **One decision, recorded.** Whoever was silent, exactly one object was designated, by people with names and a counter-signature, at a known time, on known data.
- **Silence did not block.** The escalation reached the operator's own State and both States' focal points, and when the deadline came Orbite decided instead of waiting.
- **The manoeuvre was checked twice**: its trajectory was screened before approval, and the orbit was observed after the burn.
- **The silence left a trace** — a severity-3 breach on the operator's compliance record — instead of an argument in the press.

## What it did worse

- **Orbite decided later than ESA did.** ESA accepted the designation within eight minutes, but Orbite waited for the other party's answer until its take-over deadline, 2.5 days later. When the designated operator agrees and the other side is silent, nothing is gained by waiting: the decision could stand at once. This is a design flaw the replay exposed, and the next change to the coordination rules.
- **The threshold had to be chosen.** With Orbite's default thresholds, 1 in 50,000 is already level 2 and opens a coordination on 28 August. The replay adopts, for 2019, a table that uses ESA's own manoeuvre threshold (1 in 10,000) as the level of attention, and shows both outcomes.
- **The evidence was not frozen at the take-over.** The trace shows "gel de ESC-1 refusé" (freeze refused): the escalation file is not an event the liability subsystem accepts yet. Known, and open.
- **Notices kept escalating after the decision.** SpaceX did not acknowledge the approval notice either, which opened a second escalation for the same silence. Whether that should count twice is undecided.

## What the replay fixed

Playing a real timeline on the real code found five gaps between subsystems that unit tests with doubles had missed, now fixed at the source:

- the take-over deadline used the *longest* notice of the two objects — 72 h for electric propulsion, a century for an object with none — so Orbite took over at once; it now uses the shortest notice among the objects able to manoeuvre;
- every unacknowledged message, including the escalation's own reminders, climbed one level: 1 → 4 in 30 minutes;
- the rule "start of burn + notice ≤ closest approach" forbade a burn half an orbit before it; the notice now runs from the decision, as the rules of the road define it;
- a take-over handled in time was declared overdue, with a severity-3 incident;
- a breach notified to a State without a channel created a new breach, in a loop — about 1,650 in three simulated days.

## Sources

- [SpaceNews, 2 September 2019](https://spacenews.com/?p=93289) and [Space.com](https://www.space.com/spacex-starlink-esa-satellite-collision-avoidance.html): ESA's manoeuvre, the 1-in-1,000 estimate, ESA's account.
- [Behind the Black: SpaceX's explanation](https://behindtheblack.com/behind-the-black/points-of-information/spacex-issues-explanation-for-nonresponse-in-potential-satellite-collision-issue): the 28 August e-mail at 1 in 50,000 and the paging bug.
- [Time](https://time.com/5667047/spacex-satellite-collision), [CBC](https://www.cbc.ca/1.5269613): contemporary coverage.

The replay's code: [test_aeolus.cell](/cases/orbite/src/test_aeolus.cell) — run it with `soma test test_aeolus.cell` in the [published source](/cases/orbite/README.md).

---
title: Aeolus and Starlink 44, replayed in Orbite
full_title: Aeolus vs Starlink 44 (2019), replayed in Orbite — what a shared protocol would have done, line by line
description: In September 2019 ESA's Aeolus manoeuvred alone to avoid Starlink 44 after its messages went unanswered. Orbite, a space traffic coordination system written in Soma, replays the incident on its real code and settles the designation three days before the closest approach — after one rule changed.
type: TechArticle
toc: yes
---

# Aeolus and Starlink 44, replayed in Orbite

<p class="lede">On 2 September 2019, ESA's Earth-observation satellite Aeolus fired its thrusters to avoid Starlink 44. The risk of collision had climbed to about 1 in 1,000, ten times ESA's threshold for acting, and ESA could not get an answer from SpaceX. There was no rule saying who should move. We replayed the incident in <a href="/orbite">Orbite</a>, a simulated coordination protocol written in Soma — on its real code, not on a model — to see what a shared protocol would have done, line by line. The first replay showed Orbite deciding <em>later</em> than ESA did; we changed one rule, and it now settles the question three days before the closest approach.</p>

## What happened

As reported by both parties at the time:

- **28 August 2019** — a conjunction alert between Aeolus and Starlink 44, with a probability of collision of about 1 in 50,000, below the threshold for a manoeuvre. In an e-mail exchange SpaceX said it did not plan to move at that level of risk.
- **The following days** — the probability rose to about 1 in 1,000, ten times ESA's threshold (1 in 10,000). ESA sought to coordinate and got no answer. SpaceX later explained that a bug in its on-call paging system kept its operator from seeing the follow-up messages, and that it would otherwise have coordinated.
- **2 September 2019** — Aeolus raised its orbit, about half an orbit before the closest approach, and passed above Starlink 44.
- No international rule said which of the two should manoeuvre.

The accounts differ on how hard each side tried to reach the other once the threshold was crossed. The replay does not settle that; it asks what happens when one side is silent, for whatever reason.

## What Orbite would have done

The replay is a test, [`test_aeolus.cell`](/cases/orbite/src/test_aeolus.cell): 101 assertions on Orbite's real bodies, no test doubles, driven by Orbite's own clock. Only the four facts above are historical. Everything else — Orbite itself (nothing like it existed in 2019), its fictional States, people, rules of the road and thresholds, the exact hours (alert on 28 August at 10:02, rise on 30 August at 14:02, closest approach on 2 September at 12:00 UTC), distances and Δv — is simulated. Aeolus has impulsive propulsion; Starlink has low-thrust electric propulsion, which needs days of notice to manoeuvre.

| When (UTC) | What Orbite does |
|---|---|
| 28 Aug, 10:02 | Conjunction opened at 1 in 50,000: level 1. No coordination, no designation. |
| 30 Aug, 14:02 | Probability up to 1 in 1,000: level 3. Coordination opened. The rules of the road propose **Aeolus**: as at sea, the more manoeuvrable vessel gives way (rule 18) — impulsive propulsion before Starlink's electric thrusters, which in any case need 72 h of notice with 70 h left. Answers are due by 14:32. |
| 30 Aug, 14:10 | ESA accepts. |
| 30 Aug, 15:00 | SpaceX has not answered, and has not refused. **Tacit agreement**: the operator who has to move said yes, the one who does not said nothing — the designation of Aeolus is settled (decision DS-CJ-1-1, one lock), 69 hours before the closest approach, and notified to both operators. Starlink 44 becomes the stand-on vessel: it keeps its course — any manoeuvre plan of its own is now refused. SpaceX's silence is recorded as a breach (late answer, severity 3). |
| 30 Aug, 15:20 | Aeolus' first plan, a 0.5 m/s burn, is **refused as timid**: screened, it leaves a probability of about 1 in 730,000, and a manoeuvre must be franc — below 1 in a million (rule 8). A 0.7 m/s plan passes: screened against the catalogue, approved with more than two and a half days of margin, and announced to SpaceX and its State — "manoeuvre at 11:15, Δv 0.7 m/s, acknowledgement expected" (rule 34). |
| 15:30 · 16:00 · 31 Aug, 01:00 | The escalation still climbs — mission lead, the focal point of SpaceX's State, then Orbite's direction and both States — but only to inform: it no longer holds the decision back. |
| 2 Sep, 11:15 | Burn of 0.7 m/s, 45 minutes — about half an orbit — before the closest approach (re-screened every ~8 h until then). |
| 2 Sep, 11:50 | Orbit observed and found conforming; the conjunction is resolved, the coordination closed, the lock retired. |

The full trace, one line per event of Orbite's journal, is in [AEOLUS_TRACE.md](/cases/orbite/src/AEOLUS_TRACE.md) (in French, like the code).

## Against 2019

In 2019 no decision was ever taken: ESA decided alone and burned on 2 September, without knowing whether Starlink 44 would move too. In Orbite the designation stands on 30 August at 15:00, three days earlier; ESA's plan is approved twenty minutes later; and the manoeuvre is no longer unilateral but a recorded decision, notified to both sides, that the other operator cannot contradict inside the protocol.

- **One decision, recorded.** Exactly one object designated, at a known time, on known data; a second designation or a plan for the other object is refused.
- **Rules every captain would recognise.** The more manoeuvrable gives way; the stand-on keeps its course; the manoeuvre is early, substantial and announced; and had ESA gone silent too after accepting, the stand-on would have been called to act in its place — though Starlink's 72-hour notice would not have allowed it here.
- **Silence did not block — and did not delay.** The first replay made ESA wait until Orbite's take-over deadline, 8 hours before the closest approach, though ESA had agreed within eight minutes. The rule we added, the **tacit agreement**: when the operator who must manoeuvre accepts and the others are only silent, with no refusal, the decision stands at the answer deadline. The escalation goes on, to inform.
- **The manoeuvre was checked twice**: its trajectory screened before approval (and again until the burn), the orbit observed after it.
- **The silence left a trace** — a severity-3 breach on the operator's compliance record — instead of an argument in the press.

## What remains

- **Notified is not informed.** SpaceX received the decision on its endpoint and never acknowledged it — its on-call system was the fault in 2019. Orbite knows it, escalates up to both States, but cannot guarantee the silent operator saw the decision. The residual risk of 2019 — that the silent satellite moves too — is reduced, not removed.
- **No four eyes on a tacit agreement.** An explicit agreement between operators never needed Orbite's counter-signature; a tacit one now does not either, because the operator who carries the burden agreed. An arbitration of a silence still takes two people. A design choice, open to discussion.
- **The threshold had to be chosen.** With Orbite's default thresholds, 1 in 50,000 is already level 2 and opens a coordination on 28 August. The replay adopts, for 2019, a table that uses ESA's own manoeuvre threshold (1 in 10,000) as the level of attention, and shows both outcomes.
- **The evidence is not frozen at a take-over** ("gel … refusé" in the first trace): the escalation file is not an event the liability subsystem accepts yet. Known, and open.

## What the replay fixed

Playing a real timeline on the real code found seven gaps between subsystems that unit tests with doubles had missed, now fixed at the source:

- the take-over deadline used the *longest* notice of the two objects — 72 h for electric propulsion, a century for an object with none — so Orbite took over at once; it now uses the shortest notice among the objects able to manoeuvre;
- every unacknowledged message, including the escalation's own reminders, climbed one level: 1 → 4 in 30 minutes;
- the rule "start of burn + notice ≤ closest approach" forbade a burn half an orbit before it; the notice now runs from the decision, as the rules of the road define it;
- a take-over handled in time was declared overdue, with a severity-3 incident;
- a breach notified to a State without a channel created a new breach, in a loop — about 1,650 in three simulated days;
- once the tacit agreement existed, a decision taken without SpaceX counted as "SpaceX answered": its escalation file closed and a new one opened every 30 minutes (about 150); answering now means "no longer silent";
- the same silence was counted as a breach several times, through undelivered messages before the deadline and after closure; those are now journaled only.

## Sources

- [SpaceNews, 2 September 2019](https://spacenews.com/?p=93289) and [Space.com](https://www.space.com/spacex-starlink-esa-satellite-collision-avoidance.html): ESA's manoeuvre, the 1-in-1,000 estimate, ESA's account.
- [Behind the Black: SpaceX's explanation](https://behindtheblack.com/behind-the-black/points-of-information/spacex-issues-explanation-for-nonresponse-in-potential-satellite-collision-issue): the 28 August e-mail at 1 in 50,000 and the paging bug.
- [Time](https://time.com/5667047/spacex-satellite-collision), [CBC](https://www.cbc.ca/1.5269613): contemporary coverage.

The replay's code: [test_aeolus.cell](/cases/orbite/src/test_aeolus.cell) — run it with `soma test test_aeolus.cell` in the [published source](/cases/orbite/README.md).

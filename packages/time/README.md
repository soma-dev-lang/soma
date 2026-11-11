# time

Unix seconds to a civil date and time in a zone and back, with daylight saving; ISO 8601 in and out. Pure: the clock is an argument.

```soma
use time

format_iso(now(), "Europe/Paris")           // "2026-03-29T03:30:00+02:00"
parse_iso("2026-03-29T03:30:00+02:00")      // Unix seconds
at_local(2026, 7, 14, 9, 0, "America/New_York")
zone_offset(zone, ts)  local_parts(ts, zone)  utc_parts(ts)  zones()
```

Zones are rules (standard offset + the EU, US or south-east Australian daylight rule, or none), not the tz database: 40 zones, present-day rules applied to every year. An unknown zone raises `UnknownZone`; `BadDate` for what is not a date.

Pure Soma. Its proofs are its tests — re-run them on your toolchain:

```
soma test packages/time/time.cell
```

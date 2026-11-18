# Foreground application summary

The common owner's lifetime reservations and the notification use different
units. `ownedCount()` remains workers plus pending restart and boot reservations;
slot admission, `ServiceLifetime.owned`, actual idle predicates and retirement
permits retain their existing meaning. A displayed application count never
releases ownership or authorizes stopping the foreground service.

`NotificationSummary` projects actor observations into disjoint active, starting
and stopping application counts. A job's actual native `stackId` groups multiple
workers into one application; an ungrouped job uses its native instance ID.
Native boot/restart admission excludes managed members, so those pending tickets
are standalone starting applications. Typed group and standalone keys cannot
collide. Stopping takes precedence over starting, then ready workers. A partially
running group is one **active** application; this does not claim every configured
part is ready.

`ServiceSupervisor.Activity` carries both the unchanged ownership count and the
immutable presentation snapshot. The same actor-turn coalescing prevents an
intermediate cancellation/reservation zero. READY and Stopping publish phase
changes even when ownership totals stay equal. Data/configuration, package,
catalog and retained IO admission/completion also update the separate background
work fact. Overlapping admin counters never become invented application or
operation totals. Presentation queries do not refresh widgets or issue more
queries, preventing a bind/read/refresh loop.

`EngineService` keeps the latest snapshot and formats English plural resources.
Only real worker/pending ownership can establish its existing foreground path;
background-only observations can update an already foreground notification but
cannot create one. A zero ownership hint still requests the actual idle handshake,
whose data/IO/catalog/binding checks remain authoritative. Explicit service start
uses a preparation caption until an actual snapshot arrives.

Twelve pure JVM cases cover native key separation, multi-worker deduplication,
phase priority, pending ticket deduplication, background-only work and snapshot
immutability. The existing native-owner data test now checks the observer,
background summary, zero guest ownership and denied retirement while a data
lease is held. The explicit Notes physical harness checks actual owner summaries
and foreground text after real starts and selective stop/restart. Compilation or
pure JVM results do not establish Android notification/FGS/device acceptance.

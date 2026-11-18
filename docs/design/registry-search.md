# Registry search observation state

Advanced public search already had a native query/download surface. Its new state
backend separates editable draft, submitted query and displayed-result query.
Each request has private identity and a monotonic epoch; query replacement,
explicit cancellation, advanced-mode disable, owner disconnection and controller
closure invalidate old replies. A stale success/failure cannot replace accepted
results or publish a report error for a newer request.

The controller retains the actual next cursor, appends unique references while
preserving already displayed rows and retries the exact failed query/cursor,
independently of later draft edits. Failed first-page searches preserve clearly
identified previous results. A disconnect after accepted results retains an
explicit first-page recovery attempt; reconnect performs no automatic fetch.
The state exposes whether Retry is actually available. Initial unavailable state
without a submitted query returns Idle on connection.

Owner connection is not evidence of Internet availability. Unavailable describes
the engine owner. Search failure remains a request failure; it does not establish
an offline cause, persistent history or an offline registry index. Cancel ends
the search observation and does not acknowledge cancellation of native HTTP work.

Technical bounds are explicit: query256 UTF-16 characters (matching the existing
editor),30 rows/page,240 retained unique rows,32 pages,4096 UTF-8 cursor bytes,
16KiB row JSON,512KiB native page input and1MiB retained row JSON. Malformed pages,
cursor cycles and overflow fail while retaining accepted rows/cursor; they do
not pretend that the registry has reached its end. Download remains explicit and
advanced packages retain the unverified warning.

AppModel adapts the existing common-owner search API and keeps request state/report
provenance. After the English conversation scheme, Store's single LazyColumn now
uses the typed pane for loading/empty/retry/next/cancel and explicit submitted/
displayed-query labels. The public unverified warning remains visible; downloaded
artifacts stay in their separate section with metadata-backed package file sizes.
No nested scrolling list or offline registry index is introduced.

Button and IME Search recheck actual current owner/loading state and submit the
latest actual draft, including input received before recomposition. Request-bound
Cancel/Retry/Next/Download callbacks also recheck the captured epoch/current row.
Retained rows remain viewable when a request fails; their action admission is
explicit. Ordinary search failure can use the existing bounded report flow.

Eighteen pure coroutine definitions passed on the app's coroutines runtime;
three additional presentation-admission definitions compile but have not executed
in this GUI batch. Actual service/registry/rotation/Internet failure, rendered
layout, keyboard and accessibility acceptance remain pending.

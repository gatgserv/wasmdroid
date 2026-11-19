# Live output and interactive input

Local implementation in progress. Host and physical-device regressions are
deferred to the integrated verification phase; compilation is not execution
evidence. The earlier shared-process cancellation failures remain documented.

## Output contract

The coordinator publishes a new generation before binding its worker. Only that
generation can acquire the single-writer lease. An old completion callback never
selects the current log by file modification time. Run order uses a monotonically
incremented metadata ordinal under the control lock.

Production stdout/stderr use direct virtual-file sinks. A write appends at most
4096 raw bytes per record and backpressures the guest on private disk I/O. There
is no unbounded intermediate stdout pipe. Separate 64 KiB memory tails provide
bounded completion snapshots; historical Stage 0 probe capture is unchanged.

Each binary record stores a sequence, wall-clock millisecond timestamp, stream
identity and byte count. The API encodes payloads as base64 so invalid UTF-8,
split code points and terminal escape sequences survive unchanged. Reader pages
are limited to 256 records and 64 KiB of payload. They support generation,
`after`, `since_ms`, `tail` and a continuation sequence. `has_more` distinguishes
an unread next page from a current EOF. `gap` exposes rotated-away history;
`incomplete_tail` exposes a crash-interrupted frame, which is never returned as
valid guest output. EOF says nothing about process/group completion.

The normal rotation target is four segments of at most 256 KiB and three runs
per instance. Separate legacy completion snapshots retain their own three-run
budget. Unknown files/directories and still-leased runs are preserved; interrupted
or failed maintenance can retain extra old storage. Disk exhaustion is an actual
write error, not silently dropped output. Files are flushed to the OS during
normal writes; no power-loss durability certification is claimed.

The advanced Logs screen follows current-generation pages, separates stdout and
stderr filters and reports truncated/interrupted history. UTF-8 decoding retains
incomplete bytes independently per stream. Its display keeps at most 128 KiB of
raw payload and 65536 rendered characters; it does not promise unlimited history.
Logs remain private and are never copied into events, analytics or automatic
reports. Native reader work runs outside the supervisor actor/UI thread.

## Native stdin and TTY

The driver now wires `stdin_open` to a bounded private virtual input file and
`tty` to a WASIX terminal bridge. It never inherits host stdin, terminal settings,
PTY descriptors or host shell commands. A closed-input specification receives an
explicit null input file. Guest terminal state controls echo/canonical mode;
resizing updates the virtual size and sends the guest window-change signal.

Input has a 64 KiB byte queue, 4096-byte request cap, sixteen native control
requests and a conservative 32 KiB canonical line bound. Upstream WASIX line
discipline handles canonical editing, Unicode fragments and signals. Its stdin
sink accepts an entire write or waits for capacity, because upstream canonical
writes do not retry a short successful write. Echo output also completes each
write. Closing input drains queued bytes and then delivers permanent EOF; reopening
requires another service start. This explicit End input operation is separate
from sending a raw Ctrl+D byte in a guest-selected raw mode.

Input waits outside the execution mutex. Poll, interrupt and stop remain
independent of a blocked stdin write. Android IPC carries an opaque request ID
and the exact worker generation; the common controller permits only one stdin
command at a time and rejects stale attachments. The worker bounds concurrent
control handlers, keeps the main Binder Looper free, and never logs input bytes.
Worker death fails pending input commands. An acknowledgement means that the
terminal event was processed, not that the application consumed it or completed
a command; a failure after partial processing must not be retried automatically.

The terminal GUI scheme was presented before implementation: terminal viewport,
cursor, keyboard/text input, Esc/Tab/arrows/interrupt/EOF controls. The native
emulator/keyboard/attach frontend is now implemented locally. Python/Bash TTY
behavior, password echo, resize, input backpressure, stale-generation races and
other-service isolation still need actual runtime acceptance.

## Native display and attachment

The engine pins MIT-licensed `vt100` 0.18.1 (MSRV 1.85, compatible with our pinned
Rust 1.95). `default-features = false` keeps vte 0.15.0's fixed 1024-byte OSC buffer
instead of its allocating std implementation. Both the neutral and Android Wasmer
feature trees were inspected. Cells have at most 22 bytes of text; complex
combining sequences beyond that upstream limit are not fully represented.
Clipboard, title and guest-requested window resize callbacks are no-ops.

The worker feeds stdout/stderr into one live terminal model, with separate raw
logs unchanged. Virtual line-feed processing affects rendering. Display state
continues while the UI is detached; reopening reads the current screen rather
than reconstructing a cursor from a rotated-away log prefix. Bounds are 128 rows,
256 columns and 256 scrollback rows, plus one immutable display snapshot.

Snapshot tokens keep pages coherent while guest output continues. Each response
contains at most 16 rows and 256 KiB of UTF-8 JSON, leaving room for Android's
UTF-16 Binder representation and message metadata. An unchanged version produces
only a small acknowledgement. Device status/cursor/size queries receive bounded
virtual replies through stdin; reply pressure is surfaced rather than allocating
unbounded protocol data. No response reaches a host terminal or Android API.

Normal guest completion optionally persists final display pages in the private
log generation directory. An index is published last; partial persistence is not
reported as a complete snapshot. Cache failure does not invent a failed guest
exit. Read-only final display pages are available after the worker dies. They are
pruned with the identified log run and excluded from per-volume data backups.
Forced death can leave only raw logs and the last frame already held by the UI.

The advanced Compose screen embeds a native Canvas grid/IME view, with Unicode
and wide cells, indexed/RGB colors, attributes, alternate screen and cursor state.
It supports hardware keys, mobile composition, Esc/Tab/arrows, one-shot Ctrl/Alt,
explicit interrupt/EOF, history, viewport panning and manual copy/paste. Copy marks
clipboard text sensitive; clipboard access happens only on a user action. Guest
OSC output never invokes clipboard access. Paste is bounded and rejects embedded
terminal control characters; wrapping follows the last displayed bracketed-paste
mode. There is no automatic retry after ambiguous input failure.

The attachment saves only its opaque generation across activity recreation. A
different worker generation cannot receive its keystrokes. Detaching cancels
unsent UI input and leaves the guest running. Resizing updates only dimensions,
without replaying stale echo/canonical flags over a guest's concurrent change.
The input bridge still represents WASIX terminal semantics; it is not a claim of
complete host POSIX PTY/xterm compatibility. Resize uses upstream truncation rather
than a full scrollback reflow. Physical IME, rotation, accessibility, clipboard,
rendering and real interactive Python/Bash/SQLite acceptance remain pending.

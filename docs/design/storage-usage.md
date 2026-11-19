# Read-only storage observations

The neutral backend now captures the common owner's trusted directory capability
and ownership lease, then scans outside the facade/session mutex. It reads only
fixed categories: downloaded CAS files (`store/blobs/sha256`), startup cache
(`compiled-cache`), saved volume data (`volumes/data`) and logs (`logs`). It does
not create category directories or return names, paths or file contents.

The metric is observed logical regular-file length per directory entry. Sparse
files retain their logical lengths; hard links may count more than once. Browser
profiles, metadata databases, APKs and other app/device files are excluded. This
is not total app disk usage, allocated blocks, a quota or a space reservation.
Concurrent writes, rename and deletion mean even a completed walk is non-atomic;
partial values are not guaranteed lower bounds of current usage.

Each category reports Complete, Partial or Unavailable, optional observed bytes,
counts and closed issue codes. Missing unused directories are observed empty;
an unreadable, unsafe or unscanned category is not fabricated as zero. The checked
aggregate covers only the named categories and is null if a category is unavailable
or arithmetic overflows. The caller can inspect completion independently of an
observed aggregate.

Technical bounds are explicit rather than a product quota: at most65,536 visited
entries, depth32 and an elapsed check between filesystem calls. The FFI chooses
2 seconds; a blocking kernel metadata call cannot be forcibly interrupted by
that elapsed check. The core also accepts an atomic cancellation signal. Unix/
Android enumerates through held directory descriptors, nofollow metadata and
single-component nofollow child opens. The existing cap-primitives dependency is
now direct, with the same locked4.0.3 version/checksum.

Independent review found that this library reconstructs ambient paths for Windows
enumeration. Non-Unix snapshots therefore return fixed Unavailable /
platform_unsupported results without enumerating categories. No insecure fallback
or platform-independent guarantee is inferred from passing Windows byte-count
fixtures. This does not disable the Android/Unix implementation.

The common Android owner coalesces one actual scan and gives each caller a distinct
observer. Cancelling/completing an observer cannot release native IO ownership or
complete another caller's result. Android StatFs available bytes are sampled
separately with explicit availability status; their timing and accounting differ
from the native logical-byte scan.

After its English conversation scheme, Settings now shows counted/partial/
unavailable values, an observed category sum, separately available space and
explicit Refresh. A visible composition requests a fresh observation; there is
no periodic background scan. AppModel binds the controller to the current owner,
invalidates on disconnect and withholds old snapshots. Reconnection never revives
an old reply. Metadata-backed package file sizes also appear on downloaded
artifact cards; unknown or lossy numeric representations are not coerced.

The typed controller and unsigned binary-unit formatter are separate from the
Android JSON codec. The codec checks the exact schema/metric/categories, count/
sum/completion consistency, fixed issue codes, one root object and actual EOF.
Lossy Double values and coercive string/boolean counts are refused. Failed
observations can open the existing reviewed report flow using only static
Other/Data/Failed tokens; storage counts, raw errors and paths never enter it.

No persistent quota/default/eviction policy is selected by this source. Ten pure
state/formatter cases passed; seven actual Android JSON definitions only compile.
Physical Android, running-guest filesystem races, actual storage rendering and
accessibility acceptance remain separate from those checks/cross-compilation.

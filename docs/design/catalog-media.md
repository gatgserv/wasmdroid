# Authenticated inline catalog media

Catalog schema3 extends the existing single/stack recipe contract with display
media. Legacy schemas1/2 keep their absent-field serialization and reject media.
An unverified schema3 draft may omit media; a verified schema3 package requires
one icon and one or two screenshots. This is source implementation, not evidence
that an actual recipe or screenshot has been accepted. Production catalog stays
empty.

`media` contains `icon` and `screenshots`. Each image carries `format` (png/jpeg),
`size_bytes`, canonical SHA-256 `digest`, `width`, `height`, canonical standard
`data_base64`, accessible `alt`, and `provenance` containing license, attribution
and an inert committed-report reference. Unknown fields and explicit null media
are rejected. No image URL, external fetch, script or runtime permission is
accepted. A media change changes the whole catalog digest and installation
selection; it does not change the WebC artifact digest.

## Technical bounds and trust

Icons have at most8KiB compressed bytes and256×256 pixels. A screenshot has at
most20KiB, maximum side1280, minimum side at most720 and at most921,600 pixels.
Total decoded base64 payload bytes are bounded to48KiB per package. Alt text is
nonempty, at most240 UTF-8 bytes; license128, attribution256, report512. Controls
are rejected. Report references are inert bounded `docs/...md` paths without
traversal components; validation does not open or fetch them.

All existing serialized entry128KiB and remote-catalog4MiB/512-package limits
continue to apply cumulatively. Seed/recipe/media allowances cannot each consume
the whole entry. Decoded length, canonical base64 and actual digest must match.

Rust admits a static8-bit noninterlaced PNG subset with bounded chunk framing,
actual chunk CRCs and declared dimensions; metadata/profile/text/APNG chunks are
rejected by parsed type. JPEG support is a baseline8-bit sequential marker
envelope with limited pixel/JFIF data, no EXIF/ICC/comments/other application
payloads. These checks deliberately do not decompress PNG pixels or validate
JPEG entropy and do not certify decoder safety or screenshot truth.

Signed remote manifests require `PackageReview.media_review` exactly when a
package has media, in addition to existing Android/artifact/license attestations.
The signature binds the exact whole catalog bytes, including media digests and
provenance. Legacy absent media_review serializes unchanged; explicit null is
rejected. A nonempty report reference cannot prove its contents or the author's
review: actual source/license/image/device acceptance remains a publication gate.

The private one-entry FFI helper validates media-bearing entries through schema3
while retaining schema2 handling for legacy absent-media entries. The enclosing
accepted catalog supplies version and trust authority. Existing strict one-root
serde parsing and entry bounds remain; no new promotion path is introduced.

## Android display backend

The Android adapter checks exact bounded metadata, canonical bytes/hash and inert
provenance, separately from native catalog trust. Numeric strings or lossy Double
counts are refused. The codec requires one bounded root object and actual EOF;
legacy absent media returns no image, while explicit null is invalid.

BitmapFactory first reads actual MIME/dimensions without pixel allocation; they
must match declarations and bounded roles. A sampled software ARGB bitmap is
then decoded off-main, with scaled decoding disabled and resulting dimensions/
allocation checked. One decoder instance admits at most eight active/pending
calls and executes one native decode at a time. Its cache retains at most8MiB;
this is a cache bound, not a process-RSS guarantee or a bound on visible UI refs.
Only failed/cancelled unpublished bitmaps are recycled. Cache eviction drops
references without recycling images still held by a consumer.

GUI must share one decoder for the intended application-wide cache budget and
fence results by current catalog/entry/image identity before display. Identical
validated pixels may be shared by content digest/dimensions/sample. Cancellation
does not claim interruption of a native decoder already executing; the permit
remains held until it returns. Invalid/OOM/busy decode outcomes give a local
unavailable preview, with no network fallback.

## Scheme-backed native rendering

The English Store/details/gallery proposal was shown before this GUI change.
Store rows now optionally show the icon; details provide bounded previews,
previous/next, full preview, local retry/unavailable states and expandable image
attribution/license credits. Legacy entries omit the media section. Text remains
English and images use their validated accessible alt descriptions. Provenance
report paths stay inert and are not exposed as fetchable links.

One retained AppModel owns the shared decoder through rotation. The accepted
source has an opaque incarnation token plus exact entry bytes; this selection
travels through media parsing, bitmap completion, dialogs and installation.
Synchronous current-frame checks withhold old pixels/credits immediately, and
after-await checks reject late results even when cached pixels are identical.
Source loss/change closes full preview and requires a new Store selection before
Install. Source owner identity is checked before reads and after completion;
an old engine's delayed refresh/finally cannot publish after owner reconnect.
Disconnect invalidates the presentation token; a new accepted read mints a new
incarnation. Native exact remote sequence/digest validation remains authoritative.

Five pure payload cases and six pure selection/publication cases passed; actual
Android decoder/JSON/cache definitions require device execution. Real PNG/JPEG
provider behavior, cache eviction,
source-change/rotation/accessibility/layout, image truth/license review and actual
Store selection/install remain separate final acceptance. Generated test pixels
and contract fixtures are not approved application screenshots.

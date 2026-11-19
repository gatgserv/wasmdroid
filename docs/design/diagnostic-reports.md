# Reviewed diagnostic reports

The owner selected preview first, then a GitHub issue form. Failed application
and package operations now offer Send report. The English review dialog shows an
immutable, selectable preview before any external action. Open GitHub form opens
the fixed HTTPS `gatgserv/wasmdroid/issues/new` destination with a fixed title
and the exact URI-encoded preview as its body. The user can edit it and decides
whether to submit in their browser. The application neither authenticates to
GitHub nor creates an issue through an API. Failure to open a browser is shown
locally; no sent-success claim is made.

## Content contract

The sealed preview is bounded to1024 UTF-8 bytes and constructed only from:

- This application's generated version name/code.
- A bounded Android API level and explicitly allowlisted ABI values.
- Closed operation, observed-stage and failure-code tokens, with fixed Unknown
  or Other fallback values.

It excludes raw exceptions/errors, guest stdout/stderr, package/application IDs,
commands, environment variables, filenames/paths, URLs, network addresses,
device model/manufacturer/serial, account identity, timestamps and arbitrary
caller-provided keys. A failure token describes the observed classification;
it does not prove an underlying cause. Generic errors without typed provenance
use Other/Unknown/Failed. Package task reporting copies only approved typed
fields; it does not include its technical-error string.

The visible report is captured from normalized fields and stays fixed while it
is reviewed. No file, attachment, ClipData, raw log or hidden context accompanies
the ACTION_VIEW intent. No account token or telemetry endpoint is involved.

## Optional disclosure and acceptance

The explicit Open GitHub form action passes the displayed version/platform and
diagnostic tokens to GitHub through the user's browser. The browser/GitHub may
apply their own account and request policies; the application promises only the
content of its own intent. Submitting a public issue may make the user's edited
text public. There is no automatic submission, periodic collection or background
upload. An issue-form launch is not evidence that an issue was submitted.

Pure model checks cover normalization, content bounds, raw-input rejection and
typed provenance. The actual Android intent assertions compile but have not run
in this batch. Browser availability, displayed content, form prefill, rotation,
accessibility and explicit submission control still require final Android/UI
acceptance. Production privacy/distribution review remains separate.

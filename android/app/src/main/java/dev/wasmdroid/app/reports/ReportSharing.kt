package dev.wasmdroid.app.reports

import android.content.Intent
import android.net.Uri

/** Open the owner's selected issue form with exactly the reviewed public fields.
 * Opening the form neither submits an issue nor grants API/account authority. */
internal fun diagnosticIssueIntent(preview:DiagnosticPreview):Intent {
    require(preview.utf8Bytes in 1..DiagnosticReport.MAX_UTF8_BYTES)
    val uri=Uri.Builder().scheme("https").authority("github.com")
        .appendPath("gatgserv").appendPath("wasmdroid").appendPath("issues").appendPath("new")
        .appendQueryParameter("title","WasmDroid diagnostic report")
        .appendQueryParameter("body",preview.text).build()
    return Intent(Intent.ACTION_VIEW,uri).addCategory(Intent.CATEGORY_BROWSABLE)
}

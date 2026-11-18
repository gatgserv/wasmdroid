package dev.wasmdroid.app.ui

import android.content.ActivityNotFoundException
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import dev.wasmdroid.app.reports.*

/** Save only normalized code tokens. A later task update cannot change the
 * reviewed report, and rotation never saves the task, raw error or app identity. */
@Composable internal fun DiagnosticReportButton(issue:ReportIssue) {
    var failure by rememberSaveable{mutableStateOf<String?>(null)}
    var operation by rememberSaveable{mutableStateOf<String?>(null)}
    var stage by rememberSaveable{mutableStateOf<String?>(null)}
    TextButton(onClick={failure=issue.failure.code;operation=issue.operation.code;stage=issue.stage.code}){
        Text(stringResource(R.string.report_send))
    }
    if(failure!=null){
        DiagnosticReportDialog(ReportIssue.fromCodes(failure,operation,stage)){failure=null;operation=null;stage=null}
    }
}

@Composable private fun DiagnosticReportDialog(issue:ReportIssue,dismiss:()->Unit) {
    val context=LocalContext.current
    val preview=remember(issue){DiagnosticReport.preview(issue,ReportPlatform.current())}
    var unavailable by remember(issue){mutableStateOf(false)}
    AlertDialog(onDismissRequest=dismiss,
        title={Text(stringResource(R.string.report_review))},
        text={Column(Modifier.verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(16.dp)){
            Text(stringResource(R.string.report_review_detail))
            SelectionContainer{Text(preview.text,style=MaterialTheme.typography.bodySmall)}
            Text(stringResource(R.string.report_excludes),style=MaterialTheme.typography.bodySmall)
            if(unavailable)Text(stringResource(R.string.report_share_unavailable),color=MaterialTheme.colorScheme.error)
        }},
        confirmButton={TextButton(onClick={
            // The explicit visible click is the only side effect. Opening a
            // issue form is not evidence that an issue was submitted.
            try{
                context.startActivity(diagnosticIssueIntent(preview))
                unavailable=false
            }catch(_:ActivityNotFoundException){unavailable=true}
            catch(_:SecurityException){unavailable=true}
        }){Text(stringResource(R.string.report_open_github))}},
        dismissButton={TextButton(onClick=dismiss){Text(stringResource(R.string.close))}})
}

package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Locale
import org.json.JSONObject

private val releaseDateFormat = DateTimeFormatter.ofPattern("MMM d, uuuu", Locale.ENGLISH)
    .withZone(ZoneOffset.UTC)

/** Presentation admission only. Actual resolve/download authority stays with
 * the common owner and controlled installer, never a search result's labels. */
internal fun registrySearchDownloadAllowed(state:AppState,reference:String):Boolean {
    val search=state.registrySearch
    return state.connected&&search.ownerConnected&&
        search.phase==RegistrySearchPhase.Ready&&search.resultsQuery!=null&&search.requestedQuery==search.resultsQuery&&
        search.rows.any{it.reference==reference&&it.legacyWebcVersion==null}&&!state.packageWorkActive()&&!state.packagesUnavailable
}
internal fun registrySearchSubmitAllowed(state:AppState):Boolean = state.registrySearch.let{
    state.connected&&it.ownerConnected&&!it.loading&&it.draftQuery.isNotBlank()&&
        it.draftQuery.length<=RegistrySearchController.MAX_QUERY_CHARS
}

@Composable
internal fun RegistrySearchForm(state: AppState, model: AppModel, onSubmitted: () -> Unit) {
    val focus = LocalFocusManager.current
    val submit = {
        val actual = model.state.value
        if (registrySearchSubmitAllowed(actual)) {
            focus.clearFocus()
            model.search(actual.registrySearch.draftQuery)
            onSubmitted()
        }
    }
    Row(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OutlinedTextField(
            value = state.registrySearch.draftQuery,
            onValueChange = model::setRegistryQuery,
            label = { Text(stringResource(R.string.search_packages)) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = { submit() }),
            modifier = Modifier.weight(1f),
        )
        Button(enabled = registrySearchSubmitAllowed(state), onClick = submit) {
            Text(stringResource(R.string.search))
        }
    }
}

/** Store results and recovery stay in the same scrollable list. */
@OptIn(ExperimentalLayoutApi::class)
internal fun LazyListScope.registrySearchItems(state:AppState,model:AppModel,downloadedPackages:Map<String,String>){
    val search=state.registrySearch
    if (search.loading || search.canRetry || search.requestedQuery != null && search.draftQuery != search.requestedQuery)
        item(key="registry-search:controls"){
        Column(verticalArrangement=Arrangement.spacedBy(10.dp)){
            FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalArrangement=Arrangement.spacedBy(4.dp)){
                if(search.loading)OutlinedButton(onClick={
                    val actual=model.state.value.registrySearch
                    if(actual.requestEpoch==search.requestEpoch&&actual.loading)model.cancelRegistrySearch()
                }){Text(stringResource(R.string.search_stop_waiting))}
                if(search.canRetry)OutlinedButton(enabled=state.connected,onClick={
                    val actual=model.state.value
                    if(actual.connected&&actual.registrySearch.requestEpoch==search.requestEpoch&&actual.registrySearch.canRetry)model.retryRegistrySearch()
                }){Text(stringResource(R.string.search_retry))}
            }
            search.requestedQuery?.let{Text(stringResource(R.string.search_requested_query,it),style=MaterialTheme.typography.bodySmall)}
            if(search.requestedQuery!=null&&search.draftQuery!=search.requestedQuery)
                Text(stringResource(R.string.search_draft_not_submitted),style=MaterialTheme.typography.bodySmall)
        }
    }
    if(search.phase!=RegistrySearchPhase.Ready||search.empty||!state.connected||!search.ownerConnected)item(key="registry-search:status"){
        RegistrySearchStatus(state)
    }
    if(search.resultsQuery!=null){
        item(key="registry-search:result-heading"){
            Column(verticalArrangement=Arrangement.spacedBy(6.dp)){
                Text(stringResource(R.string.search_results_query,search.resultsQuery),style=MaterialTheme.typography.titleMedium)
                if(search.showingPreviousResults)Text(stringResource(R.string.search_previous_results),style=MaterialTheme.typography.bodySmall)
                if (search.rows.isNotEmpty()) {
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        FilterChip(
                            selected = search.order == RegistrySearchOrder.Registry,
                            onClick = { model.setRegistrySearchOrder(RegistrySearchOrder.Registry) },
                            label = { Text(stringResource(R.string.search_registry_order)) },
                        )
                        FilterChip(
                            selected = search.order == RegistrySearchOrder.NewestRelease,
                            onClick = { model.setRegistrySearchOrder(RegistrySearchOrder.NewestRelease) },
                            label = { Text(stringResource(R.string.search_newest_release)) },
                        )
                        if (search.rows.any { it.legacyWebcVersion != null }) {
                            FilterChip(
                                selected = search.showIncompatible,
                                onClick = { model.setShowIncompatibleReleases(!search.showIncompatible) },
                                label = { Text(stringResource(R.string.search_show_incompatible)) },
                            )
                        }
                    }
                }
            }
        }
    }
    val displayedRows = orderRegistryResults(search.visibleRows, search.order)
    if (search.phase == RegistrySearchPhase.Ready && search.rows.isNotEmpty() && displayedRows.isEmpty()) {
        item(key = "registry-search:filtered") {
            Text(stringResource(R.string.search_legacy_webc_filtered), modifier = Modifier.semantics {
                liveRegion = LiveRegionMode.Polite
            })
        }
    }
    items(displayedRows,key={"registry-search:result:${it.reference}"}){row->
        val description=remember(row.json){runCatching{JSONObject(row.json).optString("description")}.getOrDefault("")}
        OutlinedCard(Modifier.fillMaxWidth()){
            Column(Modifier.padding(16.dp),verticalArrangement=Arrangement.spacedBy(8.dp)){
                Text(row.reference,style=MaterialTheme.typography.titleMedium)
                if (row.legacyWebcVersion == null) AndroidTestedLabel(row.reference)
                if (search.order == RegistrySearchOrder.NewestRelease) {
                    row.releasedAt?.let { releasedAt ->
                        Text(
                            stringResource(R.string.search_released_on, releaseDateFormat.format(releasedAt)),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                if(description.isNotBlank())Text(description)
                row.legacyWebcVersion?.let { version ->
                    Text(
                        stringResource(R.string.search_legacy_webc, version),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                val downloaded = downloadedPackages[row.reference]
                if (downloaded != null) {
                    Text(stringResource(R.string.downloaded), style = MaterialTheme.typography.labelLarge)
                    CreateDownloadedPackageServiceButton(downloaded, state, model)
                } else {
                    OutlinedButton(enabled=registrySearchDownloadAllowed(state,row.reference),onClick={
                        val actual=model.state.value
                        if (actual.registrySearch.requestEpoch == search.requestEpoch &&
                            registrySearchDownloadAllowed(actual,row.reference) &&
                            actual.images.none { JSONObject(it).getString("reference") == row.reference }
                        ) model.pull(row.reference)
                    }){Text(stringResource(R.string.download))}
                }
            }
        }
    }
    if(search.canNext)item(key="registry-search:next"){
        OutlinedButton(enabled=state.connected,onClick={
            val actual=model.state.value
            if(actual.connected&&actual.registrySearch.requestEpoch==search.requestEpoch&&actual.registrySearch.canNext)model.nextRegistryPage()
        }){Text(stringResource(R.string.search_load_more))}
    }
    if (search.rows.isNotEmpty()) item(key = "registry-search:compatibility") {
        Text(stringResource(R.string.registry_warning), style = MaterialTheme.typography.bodySmall)
    }
}

@Composable private fun RegistrySearchStatus(state:AppState){
    val search=state.registrySearch
    val message=when{
        !state.connected||!search.ownerConnected->R.string.search_owner_unavailable
        search.phase==RegistrySearchPhase.LoadingFirst->R.string.search_loading_first
        search.phase==RegistrySearchPhase.LoadingNext->R.string.search_loading_next
        search.phase==RegistrySearchPhase.Cancelled->R.string.search_cancelled_observation
        search.phase==RegistrySearchPhase.Unavailable->R.string.search_owner_changed
        search.phase==RegistrySearchPhase.Failed->when(search.problem){
            RegistrySearchProblem.InvalidQuery->R.string.search_invalid_query
            RegistrySearchProblem.InvalidPage->R.string.search_invalid_page
            RegistrySearchProblem.LimitReached->R.string.search_limit_reached
            else->R.string.search_failed
        }
        search.empty&&search.canNext->R.string.search_empty_page
        search.empty->R.string.search_no_results
        search.phase==RegistrySearchPhase.Idle->R.string.search_idle
        else->null
    }
    Column(verticalArrangement=Arrangement.spacedBy(8.dp)){
        if(message!=null)Text(stringResource(message),modifier=Modifier.semantics{liveRegion=LiveRegionMode.Polite},
            color=if(search.phase==RegistrySearchPhase.Failed)MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
        if(search.loading)LinearProgressIndicator(Modifier.fillMaxWidth())
    }
}

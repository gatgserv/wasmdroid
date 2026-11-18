package dev.wasmdroid.app.ui

internal enum class CatalogUpdateStatus { NotConfigured, InvalidConfiguration, NoSnapshot, Current, Expired, Clock, UpdateRequired, Refreshing, Failed, Unavailable }

internal data class CatalogUpdatesView(
    val status: CatalogUpdateStatus,
    val canRefresh: Boolean,
    val cancelGeneration: String?,
    val remote: Boolean,
    val installAllowed: Boolean,
) {
    companion object {
        fun from(state: AppState): CatalogUpdatesView {
            val refreshing = state.catalogRefreshing || "catalog:refresh" in state.busy
            val status = when {
                !state.connected -> CatalogUpdateStatus.Unavailable
                state.catalogUpdateConfiguration == "disabled" -> CatalogUpdateStatus.NotConfigured
                state.catalogUpdateConfiguration != "configured" -> CatalogUpdateStatus.InvalidConfiguration
                refreshing -> CatalogUpdateStatus.Refreshing
                state.catalogError -> CatalogUpdateStatus.Unavailable
                state.catalogRefreshOutcome == "failed" -> CatalogUpdateStatus.Failed
                !state.catalogRemote -> CatalogUpdateStatus.NoSnapshot
                state.catalogFreshness == "current" -> CatalogUpdateStatus.Current
                state.catalogFreshness == "expired" -> CatalogUpdateStatus.Expired
                state.catalogFreshness == "clock_before_acceptance" -> CatalogUpdateStatus.Clock
                state.catalogFreshness == "below_minimum_sequence" -> CatalogUpdateStatus.UpdateRequired
                else -> CatalogUpdateStatus.Unavailable
            }
            return CatalogUpdatesView(status,
                state.connected && state.catalogUpdateConfiguration == "configured" && !refreshing,
                state.catalogRefreshGeneration?.takeIf { refreshing && state.connected && it.isNotBlank() },
                state.catalogRemote, catalogInstallAllowed(state))
        }
    }
}

/** Presentation gate only. Native exact sequence/digest selection remains the
 * authority; refresh failure can keep a still-current last accepted snapshot. */
internal fun catalogInstallAllowed(state: AppState): Boolean = state.connected && !state.catalogError &&
    (!state.catalogRemote || (state.catalogUpdateConfiguration == "configured" && state.catalogFreshness == "current"))

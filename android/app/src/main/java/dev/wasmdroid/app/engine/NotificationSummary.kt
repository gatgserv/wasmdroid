package dev.wasmdroid.app.engine

/** Pure presentation projection, never an ownership or idle-retirement test.
 * The actor supplies one consistent snapshot using actual native membership.
 * No label, display name, recipe or PendingIntent can assign a group here. */
object NotificationSummary {
    enum class Kind { Standalone, Group }
    enum class Phase { Running, Starting, Stopping }

    data class ApplicationKey(val kind: Kind, val nativeId: String) {
        init { require(nativeId.isNotBlank()) }
    }

    /** A worker or pending boot/restart reservation for one logical app.
     * Multiple observations may intentionally refer to the same native key. */
    data class ApplicationWork(val application: ApplicationKey, val phase: Phase)

    /** Counts are disjoint. Background work is a separate fact because one
     * real operation can occupy multiple existing ownership counters. No app
     * identity, specification, display text or operation details escape here. */
    data class Snapshot(
        val runningApplications: Int,
        val startingApplications: Int,
        val stoppingApplications: Int,
        val backgroundWork: Boolean,
    ) {
        val totalApplications: Int
            get() = runningApplications + startingApplications + stoppingApplications
    }

    /** Stopping > Starting > Running matches the owner's application state
     * precedence. A group with running and starting parts is counted once in
     * Starting; an owned stopping part makes the whole app Stopping. A partial
     * group with only ready owned workers is one Running application, without
     * asserting that every configured part is running.
     *
     * Do not replace ownedCount/lifetime reservations with totalApplications.
     * This projection neither acquires/releases ownership nor grants actions. */
    fun summarize(observations: Iterable<ApplicationWork>, backgroundWork: Boolean): Snapshot {
        val applications = mutableMapOf<ApplicationKey, Phase>()
        for (observation in observations) {
            val previous = applications[observation.application]
            applications[observation.application] = when {
                previous == Phase.Stopping || observation.phase == Phase.Stopping -> Phase.Stopping
                previous == Phase.Starting || observation.phase == Phase.Starting -> Phase.Starting
                else -> Phase.Running
            }
        }
        var running = 0
        var starting = 0
        var stopping = 0
        for (phase in applications.values) {
            when (phase) {
                Phase.Running -> running++
                Phase.Starting -> starting++
                Phase.Stopping -> stopping++
            }
        }
        return Snapshot(running, starting, stopping, backgroundWork)
    }
}

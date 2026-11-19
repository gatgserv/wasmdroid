package dev.wasmdroid.app.engine

import org.json.JSONArray
import org.json.JSONObject

/** Boot is a separate explicit opt-in from restart policy. Selection is only a
 * plan: the common owner re-inspects each revision and ownership before launch. */
object BootApplicationPlan {
    data class Candidate(val id: String, val version: ULong, val replaceDead: Boolean)

    fun candidates(instances: String, managedIds: Set<String>): List<Candidate> {
        val records = JSONArray(instances)
        return (0 until records.length()).map { records.getJSONObject(it) }
            .filter { eligible(it, managedIds) }
            .map { Candidate(it.getString("id"), it.getLong("version").toULong(), it.getString("state") == "dead") }
            .distinctBy { it.id }.sortedBy { it.id }
    }

    fun valid(candidate: Candidate, record: String, managed: Boolean, owned: Boolean): Boolean {
        val current = JSONObject(record)
        return !owned && !managed && current.getString("id") == candidate.id &&
            current.getLong("version").toULong() == candidate.version &&
            eligible(current, emptySet()) && (current.getString("state") == "dead") == candidate.replaceDead
    }

    private fun eligible(record: JSONObject, managedIds: Set<String>): Boolean {
        val spec = record.getJSONObject("spec")
        return record.getString("id") !in managedIds && record.optString("driver") == "wasmer-local" &&
            spec.optString("endpoint_id", "local") == "local" &&
            spec.optBoolean("start_on_boot", false) &&
            record.getString("state") in listOf("created", "exited", "dead")
    }
}

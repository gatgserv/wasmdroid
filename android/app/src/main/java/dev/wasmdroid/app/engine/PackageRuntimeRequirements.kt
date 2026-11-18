package dev.wasmdroid.app.engine

import org.json.JSONArray
import org.json.JSONObject

/** Only declared requirements are enforced; bundled environment defaults are not prompts. */
internal object PackageRuntimeRequirements {
    const val SERVICE_LABEL = "dev.wasmdroid.required-env"
    private const val COMMANDS_LABEL = "dev.wasmdroid.command-required-env"
    private val environmentName = Regex("[A-Za-z_][A-Za-z0-9_]{0,127}")

    enum class Kind { Text, Secret, Port }

    data class Setting(val variable: String, val kind: Kind, val manual: Boolean = false) {
        fun accepts(value: String?): Boolean = when (kind) {
            Kind.Port -> value?.toIntOrNull()?.let { it in 1..65535 } == true
            Kind.Secret -> !value.isNullOrEmpty()
            Kind.Text -> !value.isNullOrBlank()
        }
    }

    fun validVariable(name: String): Boolean = environmentName.matches(name)

    fun manualNames(labels: JSONObject?): Set<String> {
        val raw = labels?.opt(SERVICE_LABEL) as? String ?: return emptySet()
        if (raw.length > 17_000) return emptySet()
        val names = runCatching { JSONArray(raw) }.getOrNull() ?: return emptySet()
        return (0 until minOf(names.length(), 128)).mapNotNull {
            (names.opt(it) as? String)?.takeIf(::validVariable)
        }.toSet()
    }

    fun settings(artifact: JSONObject?, command: String, manualNames: Set<String>): List<Setting> {
        val raw = artifact?.optJSONObject("labels")?.opt(COMMANDS_LABEL) as? String
        val options = raw?.takeIf { it.length <= 128 * 1024 }
            ?.let { runCatching { JSONObject(it) }.getOrNull() }?.optJSONArray(command)
        val declarations = (0 until minOf(options?.length() ?: 0, 32)).mapNotNull { index ->
            val declaration = options?.optJSONObject(index) ?: return@mapNotNull null
            val name = declaration.optString("name").takeIf(::validVariable) ?: return@mapNotNull null
            val kind = when (declaration.optString("kind", "text")) {
                "text" -> Kind.Text
                "secret" -> Kind.Secret
                "port" -> Kind.Port
                else -> return@mapNotNull null
            }
            Setting(name, kind)
        }
        val custom = manualNames.filter(::validVariable).map { Setting(it, Kind.Secret, manual = true) }
        return (declarations + custom).distinctBy { it.variable }
    }

    fun missing(spec: JSONObject, artifact: JSONObject?): List<Setting> {
        val environment = spec.optJSONObject("env")
        return settings(artifact, spec.optString("command"), manualNames(spec.optJSONObject("labels")))
            .filterNot { it.accepts(environment?.opt(it.variable) as? String) }
    }

    fun validate(spec: JSONObject, images: JSONArray) {
        val reference = spec.optString("artifact_ref")
        val artifact = (0 until images.length()).asSequence().map(images::getJSONObject)
            .firstOrNull { it.optString("reference") == reference }
        val missing = missing(spec, artifact)
        check(missing.isEmpty()) {
            "Complete the required package settings before starting: " + missing.joinToString { it.variable }
        }
    }
}

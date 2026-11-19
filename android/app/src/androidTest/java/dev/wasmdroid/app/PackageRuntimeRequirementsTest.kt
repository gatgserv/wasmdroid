package dev.wasmdroid.app

import dev.wasmdroid.app.engine.PackageRuntimeRequirements
import dev.wasmdroid.app.ui.ServiceConfigurationDraft
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class PackageRuntimeRequirementsTest {
    private val reference = "wasmer.io/example/server@1.0.0"
    private val artifact = JSONObject().put("reference", reference).put("labels", JSONObject()
        .put("dev.wasmdroid.command-required-env", """{"serve":[{"name":"SERVICE_PORT","kind":"port"},{"name":"TOKEN","kind":"secret"}]}"""))

    private fun spec(command: String = "serve", environment: JSONObject = JSONObject()): JSONObject =
        JSONObject().put("artifact_ref", reference).put("command", command).put("env", environment)

    @Test fun declarationsAreScopedToTheInstalledArtifactAndCommand() {
        assertEquals(listOf("SERVICE_PORT", "TOKEN"), PackageRuntimeRequirements.missing(spec(), artifact).map { it.variable })
        assertTrue(PackageRuntimeRequirements.missing(spec("other"), artifact).isEmpty())
        assertTrue(PackageRuntimeRequirements.missing(spec(), null).isEmpty())
        val failure = assertThrows(IllegalStateException::class.java) {
            PackageRuntimeRequirements.validate(spec(), JSONArray().put(artifact))
        }
        assertTrue(failure.message.orEmpty().contains("SERVICE_PORT"))
        val environment = JSONObject().put("SERVICE_PORT", "0").put("TOKEN", "synthetic-value")
        assertEquals(listOf("SERVICE_PORT"), PackageRuntimeRequirements.missing(spec(environment = environment), artifact).map { it.variable })
        environment.put("SERVICE_PORT", "8080")
        PackageRuntimeRequirements.validate(spec(environment = environment), JSONArray().put(artifact))
    }

    @Test fun manuallyRequiredValuesPersistAndAnEmptyValuePromptsAgain() {
        val raw = spec().put("labels", JSONObject().put(PackageRuntimeRequirements.SERVICE_LABEL, """["LOCAL_VALUE"]"""))
        val draft = ServiceConfigurationDraft.fromSpecification(raw.toString())
        assertEquals("LOCAL_VALUE", draft.environment.single().name)
        assertTrue(draft.environment.single().required)
        val missing = JSONObject(draft.specification())
        assertEquals(listOf("LOCAL_VALUE"), PackageRuntimeRequirements.missing(missing, null).map { it.variable })
        val complete = draft.copy(environment = draft.environment.map { it.copy(value = "saved-value") })
        val saved = JSONObject(complete.specification())
        assertTrue(PackageRuntimeRequirements.missing(saved, null).isEmpty())
        val reopened = ServiceConfigurationDraft.fromSpecification(saved.toString())
        assertEquals("saved-value", reopened.environment.single().value)
        val cleared = reopened.copy(environment = reopened.environment.map { it.copy(value = "") })
        assertEquals(listOf("LOCAL_VALUE"), PackageRuntimeRequirements.missing(JSONObject(cleared.specification()), null).map { it.variable })
    }
}

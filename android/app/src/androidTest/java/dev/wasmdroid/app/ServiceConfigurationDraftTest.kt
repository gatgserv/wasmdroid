package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.ui.*
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Codec only; native preflight/transaction and actual UI/runtime are separate. */
@RunWith(AndroidJUnit4::class)
class ServiceConfigurationDraftTest {
    private fun initial()=JSONObject().put("artifact_ref","wasmer.io/example/service@1.0.0").put("endpoint_id","local")
        .put("command","serve").put("labels",JSONObject().put("recipe_marker","preserve-me"))
        .put("restart_policy","no").put("internet_access",false)
    @Test fun exactArgumentsUnicodeEnvironmentAndUneditedRuntimeFieldsSurviveTheForm(){
        val source=initial().put("resources",JSONObject().put("memory_max_bytes",123456))
        val arguments=listOf("", "two words", "literal 'quote'", "literal\"quote", "line\nbreak")
        val draft=ServiceConfigurationDraft.fromSpecification(source.toString()).copy(name="Aplicație",arguments=arguments,
            environment=listOf(EnvironmentDraft("UNICODE","notă"),EnvironmentDraft("PRIVATE_KEY","PRIVATE_MARKER")))
        val result=JSONObject(draft.specification())
        assertEquals(arguments,(0 until result.getJSONArray("args").length()).map{result.getJSONArray("args").getString(it)})
        assertEquals("notă",result.getJSONObject("env").getString("UNICODE"))
        assertEquals("preserve-me",result.getJSONObject("labels").getString("recipe_marker"))
        assertEquals("Aplicație",result.getJSONObject("labels").getString("dev.wasmdroid.ui.name"))
        assertEquals(123456,result.getJSONObject("resources").getInt("memory_max_bytes"))
        // A preserved unsupported field still reaches native validation rather
        // than being silently cleared by opening the form.
        assertEquals("no",result.getString("restart_policy"));assertFalse(result.getBoolean("internet_access"))
        assertFalse(draft.toString().contains("PRIVATE_"));assertFalse(draft.environment.toString().contains("PRIVATE_"))
    }
    @Test fun duplicateKeysAndInvalidPortsPathsTimeoutsHavePrivateFieldErrors(){
        val draft=ServiceConfigurationDraft.fromSpecification(initial().toString())
        rejects(ConfigurationField.Environment){draft.copy(environment=listOf(EnvironmentDraft("A","one"),EnvironmentDraft("A","PRIVATE_MARKER"))).specification()}
        rejects(ConfigurationField.Environment){draft.copy(environment=listOf(EnvironmentDraft("A=B","value"))).specification()}
        rejects(ConfigurationField.Ports){draft.copy(ports=listOf(PortDraft("0"))).specification()}
        rejects(ConfigurationField.Ports){draft.copy(ports=listOf(PortDraft("8080","65536"))).specification()}
        rejects(ConfigurationField.Ports){draft.copy(ports=listOf(PortDraft("8080"),PortDraft("8080"))).specification()}
        rejects(ConfigurationField.Network){draft.copy(network="none",ports=listOf(PortDraft("8080"))).specification()}
        rejects(ConfigurationField.WorkingDirectory){draft.copy(workingDirectory="/data/../escape").specification()}
        rejects(ConfigurationField.Mounts){draft.copy(mounts=listOf(MountDraft("data","/data/",false))).specification()}
        rejects(ConfigurationField.Timeout){draft.copy(timeoutSeconds="9223372036854776").specification()}
        rejects(ConfigurationField.Arguments){draft.copy(arguments=listOf("PRIVATE_MARKER\u0000")).specification()}
    }
    @Test fun selectedVolumeReadOnlyAndAutomaticPhonePortRemainExplicit(){
        val draft=ServiceConfigurationDraft.fromSpecification(initial().toString()).copy(
            mounts=listOf(MountDraft("data","/data",true)),ports=listOf(PortDraft("8080")),timeoutSeconds="30",tty=true,stdinOpen=true)
        val result=JSONObject(draft.specification())
        val mount=result.getJSONArray("mounts").getJSONObject(0)
        assertEquals("volume",mount.getJSONObject("source").getString("type"));assertTrue(mount.getBoolean("read_only"))
        assertEquals(0,result.getJSONArray("ports").getJSONObject(0).getInt("host_port"))
        assertTrue(result.getBoolean("tty"));assertTrue(result.getBoolean("stdin_open"))
        assertEquals(30L,result.getJSONObject("resources").getLong("timeout_seconds"))
        assertFalse(result.has("start"));assertFalse(result.has("create_volume"))
    }
    @Test fun webPublicationRequiresTheActualGuestPortAndAllowsAutomaticPhonePort() {
        val draft = ServiceConfigurationDraft.fromSpecification(initial().toString())
        rejects(ConfigurationField.Ports) {
            draft.copy(ports = listOf(PortDraft("", web = true))).specification()
        }
        val web = PortDraft("80", phone = "", web = true)
        val other = PortDraft("8080", phone = "10080")
        val published = draft.copy(ports = listOf(web, other))
        val result = JSONObject(published.specification())
        assertEquals("80", result.getJSONObject("labels").getString("dev.wasmdroid.web.port"))
        assertEquals(0, result.getJSONArray("ports").getJSONObject(0).getInt("host_port"))
        assertEquals(10080, result.getJSONArray("ports").getJSONObject(1).getInt("host_port"))
        val loaded = ServiceConfigurationDraft.fromSpecification(result.toString())
        assertEquals(listOf(true, false), loaded.ports.map { it.web })
        val reassigned = loaded.copy(ports = listOf(loaded.ports.first().copy(web = false), other.copy(web = true)))
        assertEquals("8080", JSONObject(reassigned.specification()).getJSONObject("labels").getString("dev.wasmdroid.web.port"))
        val removed = JSONObject(loaded.copy(ports = listOf(other)).specification())
        assertFalse(removed.getJSONObject("labels").has("dev.wasmdroid.web.port"))
        rejects(ConfigurationField.Ports) {
            draft.copy(ports = listOf(web, other.copy(web = true))).specification()
        }
    }
    @Test fun startupPortTracksItsPublicationIndependentlyOfTheWebAndPhonePorts() {
        val label = "dev.wasmdroid.startup.port"
        val draft = ServiceConfigurationDraft.fromSpecification(initial().toString())
        val webPort = PortDraft("8080", web = true)
        val startupPort = PortDraft("9090", phone = "19090", startup = true)
        val saved = draft.copy(arguments = listOf("--root", "/public"), ports = listOf(webPort, startupPort))
        val spec = JSONObject(saved.specification())
        assertEquals("9090", spec.getJSONObject("labels").getString(label))
        assertEquals("8080", spec.getJSONObject("labels").getString("dev.wasmdroid.web.port"))
        assertEquals(19090, spec.getJSONArray("ports").getJSONObject(1).getInt("host_port"))
        val loaded = ServiceConfigurationDraft.fromSpecification(spec.toString())
        assertEquals(listOf(webPort, startupPort), loaded.ports)
        assertEquals(listOf("--root", "/public"), loaded.arguments)
        val changed = loaded.copy(ports = listOf(webPort, startupPort.copy(guest = "9091")))
        assertEquals("9091", JSONObject(changed.specification()).getJSONObject("labels").getString(label))
        val unpublished = loaded.copy(ports = listOf(webPort))
        assertFalse(JSONObject(unpublished.specification()).getJSONObject("labels").has(label))
        rejects(ConfigurationField.Ports) {
            loaded.copy(ports = listOf(webPort.copy(startup = true), startupPort)).specification()
        }
        spec.getJSONObject("labels").put(label, "9092")
        rejects(ConfigurationField.Ports) { ServiceConfigurationDraft.fromSpecification(spec.toString()) }
    }
    @Test fun coercionAndUnknownNestedRowsAreRejectedWhileUnknownRootFieldsStayForNativeValidation(){
        val source=initial()
        rejects(ConfigurationField.Document){ServiceConfigurationDraft.fromSpecification(JSONObject(source.toString()).put("args",JSONArray().put(42)).toString())}
        rejects(ConfigurationField.Document){ServiceConfigurationDraft.fromSpecification(JSONObject(source.toString()).put("tty","true").toString())}
        val mount=JSONObject().put("source",JSONObject().put("type","volume").put("source","data")).put("target","/data").put("read_only",false).put("future_field","PRIVATE_MARKER")
        rejects(ConfigurationField.Mounts){ServiceConfigurationDraft.fromSpecification(JSONObject(source.toString()).put("mounts",JSONArray().put(mount)).toString())}
        val root=JSONObject(source.toString()).put("future_field","kept-for-native-rejection")
        assertEquals("kept-for-native-rejection",JSONObject(ServiceConfigurationDraft.fromSpecification(root.toString()).specification()).getString("future_field"))
    }
    @Test fun rawDocumentBudgetPrecedesParsingAndQuotedBracketsAreOrdinaryValues(){
        rejects(ConfigurationField.Document){ServiceConfigurationDraft.document("{\"x\":"+"[".repeat(33)+"0"+"]".repeat(33)+"}")}
        rejects(ConfigurationField.Document){ServiceConfigurationDraft.document("{\"x\":\""+"x".repeat(128*1024)+"\"}")}
        rejects(ConfigurationField.Document){ServiceConfigurationDraft.document("{\"x\":["+List(17_000){"0"}.joinToString(",")+"]}")}
        val exact="\\\""+"[{},:]".repeat(40)
        assertEquals(exact,ServiceConfigurationDraft.document(JSONObject().put("x",exact).toString()).getString("x"))
        rejects(ConfigurationField.Document){ServiceConfigurationDraft.document("{\"PRIVATE_MARKER\":")}
    }
    private fun rejects(field:ConfigurationField,action:()->Unit){
        var failure:ConfigurationInputError?=null
        try{action()}catch(expected:ConfigurationInputError){failure=expected}
        assertNotNull(failure);assertEquals(field,failure!!.field);assertFalse(failure.message!!.contains("PRIVATE_"))
    }
}

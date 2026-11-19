package dev.wasmdroid.app

import android.app.KeyguardManager
import android.app.Notification
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.engine.BrowserSessionLease
import dev.wasmdroid.app.engine.EngineService
import dev.wasmdroid.app.engine.ServiceSupervisor
import dev.wasmdroid.engine.DataTransferControl
import dev.wasmdroid.engine.engineInfo
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.InetSocketAddress
import java.net.Socket
import java.net.URL
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit

/** Explicit physical acceptance definition, not executed by compilation.
 * The draft remains unverified: runtime equivalence uses advanced owner APIs,
 * not a fabricated catalog review or a bypass of curated seed admission. */
@RunWith(AndroidJUnit4::class)
class NotesRecipeAcceptanceTest {
    private val reference = "wasmer.io/wasmer/static-web-server@2.44.0"
    private val digest = "sha256:1e84109da307837d8160d23a77c36403ab44f7ec0c592b1cfe4a5bd79c968e08"
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()

    private data class OwnedGroup(val id:String, val volume:String, val target:String)
    private data class Running(val member:String, val generation:String, val lease:BrowserSessionLease)

    private fun evidence(value:String) {
        instrumentation.sendStatus(2, Bundle().apply { putString("stream", "Notes acceptance: $value\n") })
    }

    @Test fun exactNotesAssetsServeThroughCurrentOwnerVolumesAndSurviveSelectiveRestart() {
        assertEquals("Explicit -NotesLive is required; no network or guest was admitted", "true",
            InstrumentationRegistry.getArguments().getString("wasmdroidNotesLive"))
        assertTrue("Build requires the actual Wasmer runtime", engineInfo().runtimeProbeAvailable)
        assertTrue("This acceptance requires arm64", Build.SUPPORTED_ABIS.contains("arm64-v8a"))
        val context = instrumentation.targetContext
        assertFalse("Unlock the selected device before this explicit acceptance",
            context.getSystemService(KeyguardManager::class.java).isDeviceLocked)
        assertTrue("Permit WasmDroid notifications before testing the published foreground summary",
            context.getSystemService(NotificationManager::class.java).areNotificationsEnabled())
        // This path is supplied only by the test APK Gradle asset source. Missing
        // fixture integration fails; no invented/default HTML is substituted.
        val draft = JSONObject(instrumentation.context.assets.open("notes-draft.catalog.json")
            .bufferedReader(Charsets.UTF_8).use { it.readText() })
        val entry = draft.getJSONArray("packages").getJSONObject(0)
        val recipe = entry.getJSONObject("recipe")
        assertEquals(2, draft.getInt("schema_version"))
        assertFalse(entry.getBoolean("android_verified")); assertTrue(entry.isNull("evidence"))
        assertEquals(digest, entry.getString("digest"))
        assertEquals(reference, recipe.getJSONArray("artifacts").getJSONObject(0).getString("reference"))
        val asset = recipe.getJSONArray("assets").getJSONObject(0)
        val html = instrumentation.context.assets.open("index.html").use { it.readBytes() }
        assertArrayEquals(html, asset.getString("text").toByteArray(Charsets.UTF_8))
        assertEquals(html.size.toLong(), asset.getLong("size_bytes"))
        assertEquals(asset.getString("digest"), sha256(html))
        assertEquals("site", asset.getString("volume")); assertEquals("index.html", asset.getString("path"))

        val ready = CompletableFuture<ServiceSupervisor>()
        val connection = object:ServiceConnection {
            override fun onServiceConnected(name:ComponentName?, binder:IBinder?) {
                val owner = (binder as? EngineService.LocalBinder)?.engine
                if (owner == null) ready.completeExceptionally(IllegalStateException("Actual owner binding unavailable"))
                else ready.complete(owner)
            }
            override fun onServiceDisconnected(name:ComponentName?) = Unit
        }
        // Read-only binding avoids cold restoration of unrelated applications.
        assertTrue(context.bindService(Intent(context, EngineService::class.java)
            .setAction(EngineService.ACTION_SHORTCUT_BIND), connection, Context.BIND_AUTO_CREATE))
        var owner:ServiceSupervisor? = null
        val groups = mutableListOf<OwnedGroup>()
        val acknowledgedIds = mutableListOf<String>()
        val leases = mutableListOf<BrowserSessionLease>()
        var primaryFailure:Throwable? = null
        try {
            val engine = ready.get(30, TimeUnit.SECONDS); owner = engine
            assertEquals("Run only at an idle acceptance point; existing workers are not stopped", 0,
                JSONObject(engine.controllers().get(10, TimeUnit.SECONDS)).length())
            assertEquals("Finish unrelated package operations first", 0,
                JSONObject(engine.packageActivity().get(10, TimeUnit.SECONDS)).getJSONObject("active").length())
            notification(engine, 0, false)
            val previousVolumes = volumeIds(engine)
            val refusal = refuses { engine.installApplication(entry.toString(), "Unverified acceptance draft").get(30, TimeUnit.SECONDS) }
            assertTrue("The production Android-evidence gate must remain active",
                causes(refusal).any { it.message.orEmpty().contains("requires Android evidence") })
            assertEquals(previousVolumes, volumeIds(engine))
            assertFalse(entry.getBoolean("android_verified"))

            val graph = JSONObject(engine.pull(reference).get(300, TimeUnit.SECONDS))
            val installed = graph.getJSONObject("root")
            assertEquals(reference, installed.getString("reference"))
            assertEquals(digest, installed.getString("digest"))
            assertTrue(installed.getJSONArray("commands").let { values ->
                (0 until values.length()).any { values.getString(it) == "webserver" }
            })
            evidence("immutable public root verified; unverified curated gate remained closed")

            val token = UUID.randomUUID().toString()
            for (suffix in listOf("one", "two")) {
                val id = "notes-accept-$token-$suffix"
                // Record exact test identities before the call. An uncertain
                // ACK is reported for inspection, never swept by prefix.
                evidence("creating test-owned group $id")
                val createdJson = engine.createStack(id, recipe.getString("document")).get(30, TimeUnit.SECONDS)
                acknowledgedIds.add(id)
                val created = JSONObject(createdJson)
                assertEquals(id, created.getString("id"))
                val planned = created.getJSONObject("plan").getJSONObject("volumes").getJSONObject("site")
                val volume = planned.getString("name")
                assertFalse(planned.getBoolean("external"))
                assertTrue(volume.startsWith("s-$id-v-"))
                val target = JSONObject(engine.applicationTarget("group", id).get(10, TimeUnit.SECONDS))
                    .getJSONObject("target").toString()
                val group = OwnedGroup(id, volume, target); groups.add(group)
                val member = member(engine, id)
                engine.beginDataOperation(member).get(10, TimeUnit.SECONDS).use {
                    importNew(engine, volume, "index.html", html)
                    assertArrayEquals(html, export(engine, volume, "index.html"))
                    DataTransferControl().use { control ->
                        refuses { engine.beginImport(volume, "index.html", false, html.size.toULong(),
                            64uL * 1024uL, control).get(10, TimeUnit.SECONDS).close() }
                    }
                    assertArrayEquals(html, export(engine, volume, "index.html"))
                    importNew(engine, volume, "acceptance-marker.txt", "$token-$suffix".toByteArray())
                }
            }
            assertNotEquals(groups[0].volume, groups[1].volume)
            fun start(group:OwnedGroup):Running {
                val current = JSONObject(engine.shortcutResolve(group.target).get(10, TimeUnit.SECONDS))
                assertTrue(current.getBoolean("can_start"))
                engine.shortcutAction(group.target, "start", current.getString("state_token")) { true }
                    .get(180, TimeUnit.SECONDS)
                val id = member(engine, group.id)
                val endpoint = JSONObject(engine.browserEndpoint(id).get(10, TimeUnit.SECONDS))
                val generation = endpoint.getString("generation")
                val lease = engine.acquireBrowserSession(id, generation, 8080, "/").get(10, TimeUnit.SECONDS)
                leases.add(lease)
                assertTrue(lease.valid); assertEquals(generation, lease.generation)
                assertFalse(lease.completeNetworkIsolation); assertFalse(lease.exclusiveTransportGateway)
                return Running(id, generation, lease)
            }
            val first = start(groups[0]); val second = start(groups[1])
            assertNotEquals(first.lease.hostPort, second.lease.hostPort)
            assertArrayEquals(html, http(first.lease.url))
            assertArrayEquals(html, http(second.lease.url))
            assertEquals("$token-one", String(http(first.lease.url + "acceptance-marker.txt"), Charsets.UTF_8))
            assertEquals("$token-two", String(http(second.lease.url + "acceptance-marker.txt"), Charsets.UTF_8))
            notification(engine, 2, true)
            val profile = JSONObject(engine.browserProfileState(first.member).get(10, TimeUnit.SECONDS))
                .getString("profile_token")
            val revoked = CompletableFuture<Unit>(); first.lease.onRevoked { revoked.complete(Unit) }
            engine.stopStack(groups[0].id).get(60, TimeUnit.SECONDS)
            revoked.get(10, TimeUnit.SECONDS); assertFalse(first.lease.valid)
            assertFalse(first.lease.permitsOrdinaryRequest(first.lease.url))
            refuses { engine.acquireBrowserSession(first.member, first.generation, 8080, "/").get(10, TimeUnit.SECONDS) }
            assertListenerClosed(first.lease.hostPort)
            assertTrue(second.lease.valid)
            assertEquals(second.generation, JSONObject(engine.browserEndpoint(second.member).get(10, TimeUnit.SECONDS)).getString("generation"))
            assertArrayEquals(html, http(second.lease.url))
            notification(engine, 1, true)
            val restarted = start(groups[0])
            assertNotEquals(first.generation, restarted.generation)
            assertArrayEquals(html, http(restarted.lease.url))
            assertEquals("$token-one", String(http(restarted.lease.url + "acceptance-marker.txt"), Charsets.UTF_8))
            assertEquals(profile, JSONObject(engine.browserProfileState(restarted.member).get(10, TimeUnit.SECONDS)).getString("profile_token"))
            assertTrue(second.lease.valid)
            assertEquals(second.generation, JSONObject(engine.browserEndpoint(second.member).get(10, TimeUnit.SECONDS)).getString("generation"))
            notification(engine, 2, true)
            evidence("two private roots served exact Notes bytes at distinct allocated host ports; selective stop/restart preserved peer generation and data")
            evidence("owner browser lease revoked; no WebView/localStorage, full egress, stable-origin or curated seed receipt acceptance is claimed")
        } catch (failure:Throwable) {
            primaryFailure = failure; throw failure
        } finally {
            leases.forEach { it.close() }
            var cleanupFailure:Throwable? = null
            for (id in acknowledgedIds.asReversed()) {
                try {
                    val engine = requireNotNull(owner)
                    val captured = requireNotNull(groups.singleOrNull { it.id == id }) {
                        "Created group lacks captured lineage; preserve it for inspection"
                    }
                    var actual = JSONObject(engine.shortcutResolve(captured.target).get(10, TimeUnit.SECONDS))
                    assertEquals(id, actual.getJSONObject("target").getString("id"))
                    if (actual.getBoolean("can_stop")) {
                        engine.shortcutAction(captured.target, "stop", actual.getString("state_token")) { true }
                            .get(60, TimeUnit.SECONDS)
                        actual = JSONObject(engine.shortcutResolve(captured.target).get(10, TimeUnit.SECONDS))
                    }
                    assertEquals("Cleanup cannot remove an active or uncertain application", "stopped", actual.getString("state"))
                    engine.removeApplicationTarget(captured.target, actual.getString("state_token")).get(20, TimeUnit.SECONDS)
                    // Only the original captured volume is eligible after actual
                    // lineage/state/version-guarded metadata removal succeeds.
                    assertTrue(captured.volume.startsWith("s-$id-v-"))
                    engine.removeVolume(captured.volume).get(15, TimeUnit.SECONDS)
                } catch (failure:Throwable) {
                    evidence("cleanup incomplete for exact test-owned group $id; preserve and inspect its data")
                    if (cleanupFailure == null) cleanupFailure = failure else cleanupFailure.addSuppressed(failure)
                }
            }
            context.unbindService(connection)
            // Never remove shared installed images/CAS or stop any other app.
            if (cleanupFailure != null) {
                if (primaryFailure != null) primaryFailure.addSuppressed(cleanupFailure) else throw cleanupFailure
            }
        }
    }

    private fun volumeIds(owner:ServiceSupervisor):Set<String> = JSONArray(owner.volumes().get(10, TimeUnit.SECONDS)).let { values ->
        (0 until values.length()).map { values.getJSONObject(it).getString("name") }.toSet()
    }
    private fun member(owner:ServiceSupervisor, group:String):String {
        val members = JSONArray(owner.stackMembers(group).get(10, TimeUnit.SECONDS))
        assertEquals(1, members.length()); assertEquals("web", members.getJSONObject(0).getString("service"))
        return members.getJSONObject(0).getString("instance_id")
    }
    private fun importNew(owner:ServiceSupervisor, volume:String, path:String, bytes:ByteArray) {
        DataTransferControl().use { control ->
            owner.beginImport(volume, path, false, bytes.size.toULong(), 64uL * 1024uL, control)
                .get(10, TimeUnit.SECONDS).use { transfer ->
                    assertEquals(bytes.size.toULong(), transfer.write(bytes))
                    assertEquals(bytes.size.toULong(), transfer.commit())
                }
        }
    }
    private fun export(owner:ServiceSupervisor, volume:String, path:String):ByteArray =
        owner.beginExport(volume, path).get(10, TimeUnit.SECONDS).use { transfer ->
            val bytes = ByteArrayOutputStream()
            while (true) {
                val chunk = transfer.read(4096u)
                if (chunk.isEmpty()) break
                check(bytes.size() + chunk.size <= 64 * 1024); bytes.write(chunk)
            }
            bytes.toByteArray()
        }
    private fun refuses(block:()->Unit):ExecutionException {
        try { block() } catch (failure:ExecutionException) { return failure }
        throw AssertionError("Actual owner must reject this request")
    }
    private fun causes(failure:Throwable):List<Throwable> {
        val result = mutableListOf<Throwable>(); var next:Throwable? = failure
        while (next != null && result.size < 12) { result.add(next); next = next.cause }
        return result
    }
    private fun sha256(bytes:ByteArray):String = "sha256:" + MessageDigest.getInstance("SHA-256")
        .digest(bytes).joinToString("") { "%02x".format(java.util.Locale.ROOT, it.toInt() and 255) }
    private fun http(value:String):ByteArray {
        val target = URL(value)
        assertEquals("http", target.protocol); assertEquals("127.0.0.1", target.host)
        assertTrue(target.port in 1..65535)
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(60)
        var last:IOException? = null
        while (System.nanoTime() < deadline) {
            val connection = target.openConnection() as HttpURLConnection
            connection.connectTimeout = 1000; connection.readTimeout = 2000
            connection.instanceFollowRedirects = false
            connection.useCaches = false
            connection.setRequestProperty("Connection", "close")
            connection.setRequestProperty("Cache-Control", "no-cache, no-store")
            connection.setRequestProperty("Accept-Encoding", "identity")
            try {
                assertEquals("Actual guest must return HTTP200 without redirects", 200, connection.responseCode)
                return connection.inputStream.use { input ->
                    val result = ByteArrayOutputStream(); val buffer = ByteArray(4096)
                    while (true) {
                        val count = input.read(buffer); if (count < 0) break
                        check(result.size() + count <= 64 * 1024); result.write(buffer, 0, count)
                    }
                    result.toByteArray()
                }
            } catch (failure:IOException) { last = failure; Thread.sleep(100) }
            finally { connection.disconnect() }
        }
        throw AssertionError("Actual allocated guest HTTP endpoint did not become reachable", last)
    }
    private fun assertListenerClosed(port:Int) {
        try { Socket().use { it.connect(InetSocketAddress("127.0.0.1", port), 1000) } }
        catch (_:IOException) { return }
        throw AssertionError("Stopped group still accepts on its previous host listener")
    }
    private fun notification(owner:ServiceSupervisor, running:Int, requirePublished:Boolean) {
        val expected = instrumentation.targetContext.resources.getQuantityString(
            R.plurals.notification_apps_active, running, running)
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
        while (System.nanoTime() < deadline) {
            val summary = owner.notificationSummary().get(5, TimeUnit.SECONDS)
            val ownerMatches = summary.runningApplications == running && summary.totalApplications == running
            val published = !requirePublished || instrumentation.targetContext
                .getSystemService(NotificationManager::class.java).activeNotifications.any { item ->
                    (item.notification.flags and Notification.FLAG_FOREGROUND_SERVICE) != 0 &&
                        item.notification.extras.getCharSequence(Notification.EXTRA_TEXT)?.toString()
                            ?.split('·')?.any { it.trim() == expected } == true
                }
            if (ownerMatches && published) return
            Thread.sleep(100)
        }
        throw AssertionError("Actual owner/foreground notification did not report $running logical applications")
    }
}

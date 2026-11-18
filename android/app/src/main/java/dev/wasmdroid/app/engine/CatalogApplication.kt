package dev.wasmdroid.app.engine

import dev.wasmdroid.engine.EngineStorage
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

/** Runs in the common owner after image commit, even if the UI detaches.
 * Preparation failure preserves every prepared dataset; it never starts guests. */
internal fun prepareCatalogApplication(db:EngineStorage,entry:JSONObject,graph:JSONObject):String{
    val id="app-${UUID.randomUUID()}"
    if(entry.getJSONObject("recipe").optString("type")=="stack"){
        // Native preparation revalidates every installed root and publishes
        // actual group/member/data authority. Download JSON cannot select it.
        return db.prepareCatalogStack(id,entry.toString())
    }
    // Signed recipe flags are requests. Native preparation returns a separate
    // effective spec with outbound grants disabled until explicit review.
    val spec=JSONObject(db.catalogApplicationSpec(entry.toString()))
    spec.put("artifact_ref",graph.getJSONObject("root").getString("reference"))
    val labels=spec.optJSONObject("labels")?:JSONObject().also{spec.put("labels",it)}
    labels.put("dev.wasmdroid.ui.name",entry.getString("name"))
    labels.put("dev.wasmdroid.ui.package",entry.getString("id"))
    entry.optJSONObject("web")?.let{
        labels.put("dev.wasmdroid.web.port",it.getInt("instance_port").toString())
        labels.put("dev.wasmdroid.web.path",it.getString("path"))
    }
    val mounts=spec.optJSONArray("mounts")?:JSONArray();val names=linkedMapOf<String,String>()
    for(index in 0 until mounts.length()){
        val source=mounts.getJSONObject(index).getJSONObject("source")
        require(source.getString("type")=="volume"){"Catalog host paths are not permitted"}
        val volume=names.getOrPut(source.getString("source")){"data-$id-${names.size}"}
        source.put("source",volume)
    }
    db.validateServiceSpec(spec.toString())
    names.values.forEach{db.createVolume(it)}
    return db.createInstance(id,spec.toString())
}

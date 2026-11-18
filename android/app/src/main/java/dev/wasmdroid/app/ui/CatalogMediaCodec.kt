package dev.wasmdroid.app.ui

import org.json.JSONObject
import org.json.JSONTokener

/** Invoke off-main for a bounded native-validated entry, never arbitrary URLs. */
internal object CatalogMediaCodec {
    fun parsePackage(raw:String):CatalogDisplayMedia? {
        require(raw.length<=128*1024&&raw.toByteArray(Charsets.UTF_8).size<=128*1024&&'\u0000' !in raw)
        var depth=0;var quoted=false;var escaped=false
        raw.forEach{character->
            if(quoted){if(escaped)escaped=false else if(character=='\\')escaped=true else if(character=='"')quoted=false}
            else when(character){'"'->quoted=true;'[','{'->{depth++;require(depth<=16)};']','}'->{depth--;require(depth>=0)}}
        }
        require(depth==0&&!quoted)
        val tokens=JSONTokener(raw)
        val entry=tokens.nextValue();require(entry is JSONObject)
        require(tokens.nextClean()=='\u0000'&&!tokens.more())
        if(!entry.has("media"))return null
        require(!entry.isNull("media"))
        val media=entry.getJSONObject("media");keys(media,setOf("icon","screenshots"))
        val screenshots=media.getJSONArray("screenshots");require(screenshots.length() in 1..2)
        return CatalogDisplayMedia(image(media.getJSONObject("icon"),CatalogImageRole.Icon),
            (0 until screenshots.length()).map{image(screenshots.getJSONObject(it),CatalogImageRole.Screenshot)})
            .also(CatalogImagePayload::validate)
    }
    private fun image(json:JSONObject,role:CatalogImageRole):CatalogDisplayImage {
        keys(json,setOf("format","size_bytes","digest","width","height","data_base64","alt","provenance"))
        val provenance=json.getJSONObject("provenance");keys(provenance,setOf("license","attribution","report"))
        val format=CatalogImageFormat.entries.singleOrNull{it.wire==text(json,"format")}
            ?:error("Unsupported catalog image format")
        return CatalogDisplayImage(role,format,number(json,"size_bytes"),text(json,"digest"),
            number(json,"width"),number(json,"height"),text(json,"data_base64"),text(json,"alt"),
            CatalogImageProvenance(text(provenance,"license"),text(provenance,"attribution"),text(provenance,"report")))
    }
    private fun keys(json:JSONObject,expected:Set<String>){require(json.keys().asSequence().toSet()==expected)}
    private fun text(json:JSONObject,key:String)=json.get(key).let{require(it is String);it}
    private fun number(json:JSONObject,key:String):Int {
        val value=json.get(key);require(value is Int||value is Long)
        return (value as Number).toLong().also{require(it in 1..Int.MAX_VALUE.toLong())}.toInt()
    }
}

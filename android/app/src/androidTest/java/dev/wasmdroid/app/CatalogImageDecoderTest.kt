package dev.wasmdroid.app

import android.graphics.Bitmap
import dev.wasmdroid.app.ui.*
import java.io.ByteArrayOutputStream
import java.security.MessageDigest
import java.util.Base64
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/** Actual Android decoder/JSON definitions: compile now, execute only in final
 * device acceptance. Generated pixels are not product screenshot evidence. */
class CatalogImageDecoderTest {
    private fun image()=CatalogDisplayImage(CatalogImageRole.Icon,CatalogImageFormat.Png,68,
        "sha256:43739c566e26fd7cb88f69d3864ea34740372f5ee99acac169e090beffbce5c6",1,1,
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=",
        "Synthetic test pixel",CatalogImageProvenance("LicenseRef-TestFixture","First-party contract fixture",
            "docs/verification/media-fixture-only.md"))

    @Test fun actualDecoderChecksDimensionsAndReusesOnlyValidatedPixels()=runBlocking {
        val decoder=CatalogImageDecoder();val image=image()
        val first=decoder.decode(image,64,64) as CatalogImageDecode.Ready
        assertEquals(1,first.bitmap.width);assertEquals(1,first.bitmap.height)
        assertEquals(Bitmap.Config.ARGB_8888,first.bitmap.config)
        val again=decoder.decode(image,64,64) as CatalogImageDecode.Ready
        assertSame(first.bitmap,again.bitmap);assertFalse(first.bitmap.isRecycled)
        assertEquals(CatalogImageDecode.Unavailable,decoder.decode(image.copy(width=2),64,64))
        assertEquals(CatalogImageDecode.Unavailable,decoder.decode(image.copy(digest="sha256:"+"0".repeat(64)),64,64))
        val pixel=Bitmap.createBitmap(1,1,Bitmap.Config.ARGB_8888)
        val jpeg=try{
            pixel.eraseColor(0xff778899.toInt())
            ByteArrayOutputStream().use{stream->check(pixel.compress(Bitmap.CompressFormat.JPEG,90,stream));stream.toByteArray()}
        }finally{pixel.recycle()}
        val digest=MessageDigest.getInstance("SHA-256").digest(jpeg).joinToString(""){
            (it.toInt() and 255).toString(16).padStart(2,'0')}
        val decoded=decoder.decode(image.copy(format=CatalogImageFormat.Jpeg,sizeBytes=jpeg.size,
            digest="sha256:$digest",encoded=Base64.getEncoder().encodeToString(jpeg)),64,64)
        assertTrue(decoded is CatalogImageDecode.Ready)
    }
    @Test fun cacheEvictionNeverRecyclesPixelsStillHeldByAVisibleConsumer()=runBlocking {
        fun solid(color:Int):CatalogDisplayImage {
            val bitmap=Bitmap.createBitmap(720,1280,Bitmap.Config.ARGB_8888)
            val bytes=try{
                bitmap.eraseColor(color)
                ByteArrayOutputStream().use{stream->check(bitmap.compress(Bitmap.CompressFormat.PNG,100,stream));stream.toByteArray()}
            }finally{bitmap.recycle()}
            val digest=MessageDigest.getInstance("SHA-256").digest(bytes).joinToString(""){
                (it.toInt() and 255).toString(16).padStart(2,'0')}
            return image().copy(role=CatalogImageRole.Screenshot,sizeBytes=bytes.size,digest="sha256:$digest",
                width=720,height=1280,encoded=Base64.getEncoder().encodeToString(bytes))
        }
        val decoder=CatalogImageDecoder()
        val first=(decoder.decode(solid(0xffaa0000.toInt()),720,1280) as CatalogImageDecode.Ready).bitmap
        assertTrue(decoder.decode(solid(0xff00aa00.toInt()),720,1280) is CatalogImageDecode.Ready)
        assertTrue(decoder.decode(solid(0xff0000aa.toInt()),720,1280) is CatalogImageDecode.Ready)
        assertFalse(first.isRecycled)
        assertEquals(0xffaa0000.toInt(),first.getPixel(0,0))
    }
    @Test fun actualCodecKeepsAbsentLegacyMediaButRejectsTrailingOrCoerciveFrames() {
        assertNull(CatalogMediaCodec.parsePackage("{}"))
        fun reject(raw:String){try{CatalogMediaCodec.parsePackage(raw);fail("Malformed media was accepted")}catch(_:Exception){}}
        reject("{} {}");reject("{} true");reject("{}\u0000")
        reject("{\"media\":null}")
        reject("{\"media\":{\"icon_url\":\"https://example.com/pixel.png\",\"screenshots\":[]}}")
        val source=image()
        val image=JSONObject().put("format","png").put("size_bytes",68).put("digest",source.digest)
            .put("width",1).put("height",1).put("data_base64",source.encoded).put("alt",source.alt)
            .put("provenance",JSONObject().put("license",source.provenance.license)
                .put("attribution",source.provenance.attribution).put("report",source.provenance.report))
        val media=JSONObject().put("icon",image).put("screenshots",org.json.JSONArray().put(image))
        val entry=JSONObject().put("media",media)
        assertNotNull(CatalogMediaCodec.parsePackage(entry.toString()))
        image.put("size_bytes","68");reject(entry.toString())
    }
}

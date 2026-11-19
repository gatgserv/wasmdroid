package dev.wasmdroid.app.ui

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlin.coroutines.coroutineContext

internal sealed interface CatalogImageDecode {
    /** Shared immutable bitmap. Eviction drops cache references, never recycles
     * pixels that a visible Compose image may still use. */
    data class Ready(val bitmap:Bitmap):CatalogImageDecode
    data object Unavailable:CatalogImageDecode
    data object Busy:CatalogImageDecode
}

/** One active native decoder and eight admitted calls including pending ones.
 * The eight-MiB cache bounds retained cache bytes, not process RSS or UI leases. */
internal class CatalogImageDecoder {
    private val admission=Semaphore(8)
    private val decoder=Semaphore(1)
    private data class Key(val digest:String,val format:CatalogImageFormat,val width:Int,val height:Int,val sample:Int)
    private val cache=linkedMapOf<Key,Bitmap>()
    private var cachedBytes=0L

    suspend fun decode(image:CatalogDisplayImage,targetWidth:Int,targetHeight:Int):CatalogImageDecode {
        require(targetWidth in 1..1280&&targetHeight in 1..1280)
        if(!admission.tryAcquire())return CatalogImageDecode.Busy
        try {
            return withContext(Dispatchers.Default){decoder.withPermit {
                coroutineContext.ensureActive()
                var unpublished:Bitmap?=null
                try {
                    val bytes=CatalogImagePayload.bytes(image)
                    val bounds=BitmapFactory.Options().apply{inJustDecodeBounds=true;inScaled=false}
                    BitmapFactory.decodeByteArray(bytes,0,bytes.size,bounds)
                    require(bounds.outWidth==image.width&&bounds.outHeight==image.height&&bounds.outMimeType==image.format.mime)
                    var sample=1
                    while((image.width+sample-1)/sample>targetWidth||(image.height+sample-1)/sample>targetHeight)sample*=2
                    val key=Key(image.digest,image.format,image.width,image.height,sample)
                    cache.remove(key)?.let{bitmap->cache[key]=bitmap;return@withPermit CatalogImageDecode.Ready(bitmap)}
                    coroutineContext.ensureActive()
                    val bitmap=BitmapFactory.decodeByteArray(bytes,0,bytes.size,BitmapFactory.Options().apply{
                        inScaled=false;inSampleSize=sample;inPreferredConfig=Bitmap.Config.ARGB_8888
                    })?:return@withPermit CatalogImageDecode.Unavailable
                    unpublished=bitmap
                    require(bitmap.config==Bitmap.Config.ARGB_8888)
                    require(bitmap.width in 1..(image.width+sample-1)/sample&&bitmap.height in 1..(image.height+sample-1)/sample)
                    val allocation=bitmap.allocationByteCount.toLong()
                    require(allocation in 1..MAX_BITMAP_BYTES)
                    coroutineContext.ensureActive()
                    while(cachedBytes+allocation>MAX_CACHE_BYTES&&cache.isNotEmpty()){
                        val first=cache.entries.first();cachedBytes-=first.value.allocationByteCount.toLong();cache.remove(first.key)
                    }
                    cache[key]=bitmap;cachedBytes+=allocation;unpublished=null
                    CatalogImageDecode.Ready(bitmap)
                } catch(failure:CancellationException){throw failure}
                catch(_:OutOfMemoryError){cache.clear();cachedBytes=0;CatalogImageDecode.Unavailable}
                catch(_:Exception){CatalogImageDecode.Unavailable}
                finally{unpublished?.recycle()}
            }}
        } finally{admission.release()}
    }
    companion object {
        private const val MAX_BITMAP_BYTES=4L*1024*1024
        private const val MAX_CACHE_BYTES=8L*1024*1024
    }
}

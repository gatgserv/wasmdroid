package dev.wasmdroid.app.ui

import android.util.Base64
import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.CharBuffer
import java.nio.charset.CodingErrorAction

/** Keeps raw bytes until rendering so UTF-8 split across writes is preserved.
 * No input text, environment values or logs are written to diagnostics. */
internal class LogBuffer {
    private data class Chunk(val stream:String,val bytes:ByteArray)
    private val chunks=ArrayDeque<Chunk>()
    private var bytes=0
    var generation:String?=null; private set
    var cursor=0L; private set
    var historyMissing=false; private set
    var incomplete=false; private set
    fun clear(){chunks.clear();bytes=0;cursor=0;generation=null;historyMissing=false;incomplete=false}
    fun accept(page:JSONObject){
        val next=page.getString("generation")
        if(generation!=null&&next!=generation){clear();generation=next;return}
        generation=next
        if(page.getBoolean("gap")){chunks.clear();bytes=0;historyMissing=true}
        val records=page.getJSONArray("records")
        require(records.length()<=256){"Log page exceeds limit"}
        for(index in 0 until records.length()){
            val record=records.getJSONObject(index)
            val stream=record.getString("stream");require(stream=="stdout"||stream=="stderr"){"Unknown log stream"}
            val encoded=record.getString("data_base64");require(encoded.length<=5500){"Log record exceeds limit"}
            val data=Base64.decode(encoded,Base64.NO_WRAP);require(data.size<=4096){"Log record exceeds limit"}
            chunks.addLast(Chunk(stream,data));bytes+=data.size
            while(bytes>128*1024){bytes-=chunks.removeFirst().bytes.size;historyMissing=true}
        }
        cursor=page.getLong("next_sequence");require(cursor>=0){"Log cursor exceeds platform range"}
        incomplete=page.getBoolean("incomplete_tail")
    }
    fun text(filter:String):String{
        val output=StringBuilder();val decoders=mapOf("stdout" to Utf8Decoder(),"stderr" to Utf8Decoder())
        for(chunk in chunks){if(filter=="both"||filter==chunk.stream)output.append(decoders.getValue(chunk.stream).decode(chunk.bytes))}
        if(output.length>65536){historyMissing=true;var start=output.length-65536;if(output[start].isLowSurrogate()&&start>0&&output[start-1].isHighSurrogate())start++;return output.substring(start)}
        return output.toString()
    }
}
private class Utf8Decoder {
    private val decoder=Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPLACE).onUnmappableCharacter(CodingErrorAction.REPLACE)
    private var pending=ByteArray(0)
    fun decode(bytes:ByteArray):String{
        val source=ByteBuffer.wrap(pending+bytes);val output=CharBuffer.allocate(source.remaining()+2)
        decoder.decode(source,output,false)
        pending=ByteArray(source.remaining()).also{source.get(it)}
        output.flip();return output.toString()
    }
}

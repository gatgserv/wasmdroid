package dev.wasmdroid.app

import android.util.Base64
import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.ui.LogBuffer
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Decoder/cursor regressions only; not guest execution evidence. */
@RunWith(AndroidJUnit4::class)
class LiveOutputTest {
    private fun page(generation:String,after:Long,stream:String,bytes:ByteArray,gap:Boolean=false)=JSONObject()
        .put("generation",generation).put("next_sequence",after).put("gap",gap).put("incomplete_tail",false)
        .put("records",JSONArray().put(JSONObject().put("sequence",after).put("stream",stream).put("data_base64",Base64.encodeToString(bytes,Base64.NO_WRAP))))
    @Test fun utf8FragmentsAreKeptIndependentlyPerStream(){
        val buffer=LogBuffer();val text="notă".toByteArray(Charsets.UTF_8)
        buffer.accept(page("one",1,"stdout",text.copyOfRange(0,text.size-1)))
        buffer.accept(page("one",2,"stderr","error\n".toByteArray()))
        buffer.accept(page("one",3,"stdout",text.copyOfRange(text.size-1,text.size)))
        assertEquals("notă",buffer.text("stdout"));assertEquals("error\n",buffer.text("stderr"))
        assertFalse(buffer.text("both").contains('\uFFFD'))
    }
    @Test fun aRestartNeverCombinesOutputOrReusesTheOldCursor(){
        val buffer=LogBuffer();buffer.accept(page("one",99,"stdout","old".toByteArray()))
        val next=page("two",1,"stdout","new".toByteArray());buffer.accept(next)
        assertEquals(0L,buffer.cursor);assertEquals("",buffer.text("both"))
        buffer.accept(next);assertEquals(1L,buffer.cursor);assertEquals("new",buffer.text("both"))
        buffer.accept(page("two",10,"stdout","after gap".toByteArray(),true))
        assertTrue(buffer.historyMissing);assertEquals("after gap",buffer.text("both"))
    }
}

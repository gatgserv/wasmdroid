package dev.wasmdroid.app

import android.view.KeyEvent
import android.view.inputmethod.EditorInfo
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.ui.TerminalView
import dev.wasmdroid.app.ui.TerminalFrame
import dev.wasmdroid.app.ui.TerminalCell
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Native IME/key encoding only; this is not guest/TTY or visual evidence. */
@RunWith(AndroidJUnit4::class)
class TerminalKeyboardTest {
    @Test fun imeCompositionCommitsUnicodeOnceAndTerminalKeysStayRaw(){
        val instrumentation=InstrumentationRegistry.getInstrumentation()
        instrumentation.runOnMainSync{
            val view=TerminalView(instrumentation.targetContext);view.inputEnabled=true
            val bytes=java.io.ByteArrayOutputStream();view.onInput={bytes.write(it)}
            val connection=view.onCreateInputConnection(EditorInfo())!!
            connection.setComposingText("n",1);connection.setComposingText("not",1);connection.commitText("notă",1);connection.finishComposingText()
            assertEquals("notă",bytes.toString("UTF-8"));bytes.reset()
            val empty=TerminalCell("",-1,-1,0,1)
            view.update(TerminalFrame(1,1,2,4,List(2){List(4){empty}},0,0,false,true,false,0,0,false,false))
            view.onKeyDown(KeyEvent.KEYCODE_DPAD_UP,KeyEvent(KeyEvent.ACTION_DOWN,KeyEvent.KEYCODE_DPAD_UP))
            assertArrayEquals("\u001bOA".toByteArray(),bytes.toByteArray());bytes.reset()
            view.controlLatched=true;view.onModifiersConsumed={view.controlLatched=false};connection.commitText("c",1)
            assertArrayEquals(byteArrayOf(3),bytes.toByteArray());assertFalse(view.controlLatched);bytes.reset()
            connection.deleteSurroundingText(1,0);assertArrayEquals(byteArrayOf(127),bytes.toByteArray())
            var bounded=false;view.onResize={bounded=it.cols in 1..256&&it.rows in 1..128};view.layout(0,0,400,200);assertTrue(bounded)
        }
    }
}

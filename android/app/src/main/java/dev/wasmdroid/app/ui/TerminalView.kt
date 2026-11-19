package dev.wasmdroid.app.ui

import android.content.Context
import android.graphics.*
import android.text.InputType
import android.util.TypedValue
import android.view.*
import android.view.inputmethod.*
import android.view.accessibility.AccessibilityNodeInfo
import androidx.core.graphics.withClip
import androidx.core.graphics.withTranslation
import kotlin.math.*

/** Native grid + IME. Guest escape sequences never reach a WebView, URI
 * handler, clipboard or Android JavaScript bridge. */
internal class TerminalView(context:Context):View(context){
    var onInput:(ByteArray)->Unit={}
    var onResize:(TerminalRect)->Unit={}
    var onScroll:(Int)->Unit={}
    var onModifiersConsumed:()->Unit={}
    var onInputError:(String)->Unit={}
    var controlLatched=false
    var altLatched=false
    var inputEnabled=false
        set(value){if(field==value)return;field=value;if(!value)(context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(windowToken,0)}
    private var frame:TerminalFrame?=null
    private val paint=Paint(Paint.ANTI_ALIAS_FLAG)
    private val fonts=Array(4){Typeface.create(Typeface.MONOSPACE,it)}
    private var cellWidth=1f
    private var lineHeight=1f
    private var baseline=1f
    private var downY=0f
    private var lastY=0f
    private var dragged=false
    private var downX=0f
    private var lastX=0f
    private var panX=0f
    private var panY=0f
    private var horizontalDrag=false
    private var pendingAccent=0
    init{
        isFocusable=true;isFocusableInTouchMode=true;importantForAccessibility=IMPORTANT_FOR_ACCESSIBILITY_YES
        paint.typeface=fonts[0];paint.textSize=TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP,14f,resources.displayMetrics)
        cellWidth=ceil(paint.measureText("M")).coerceAtLeast(1f)
        val metrics=paint.fontMetrics;lineHeight=ceil(metrics.descent-metrics.ascent+2);baseline=-metrics.ascent+1
        setBackgroundColor(DEFAULT_BACKGROUND)
    }
    fun update(value:TerminalFrame?){
        val old=frame;frame=value
        if(value!=null&&(old==null||old.version!=value.version||old.scrollback!=value.scrollback||old.rows!=value.rows||old.cols!=value.cols)){
            panX=panX.coerceIn(0f,maxOf(0f,value.cols*cellWidth-width));panY=panY.coerceIn(0f,maxOf(0f,value.rows*lineHeight-height))
            if(old==null){panY=maxOf(0f,(value.cursorRow+1)*lineHeight-height).coerceAtMost(maxOf(0f,value.rows*lineHeight-height))}
            contentDescription=value.plainText().takeLast(32768);invalidate()
        }
    }
    fun showKeyboard() {
        if (!inputEnabled) return
        requestFocus()
        context.getSystemService(InputMethodManager::class.java).showSoftInput(this, 0)
    }
    override fun onCheckIsTextEditor()=true
    override fun onCreateInputConnection(info:EditorInfo):InputConnection?{
        if(!inputEnabled)return null
        info.inputType=InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or InputType.TYPE_TEXT_FLAG_MULTI_LINE
        info.imeOptions=EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_FLAG_NO_FULLSCREEN or EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_ACTION_NONE
        return object:BaseInputConnection(this,false){
            private var composing=""
            override fun commitText(text:CharSequence?,newCursorPosition:Int):Boolean{val value=text?.toString()?:"";if(value=="\n"||value=="\r")finishComposingText();composing="";commit(if(value=="\n")"\r"else value);return true}
            override fun setComposingText(text:CharSequence?,newCursorPosition:Int):Boolean{val value=text?.toString()?:"";if(value.toByteArray(Charsets.UTF_8).size>64*1024){onInputError("IME composition exceeds 64 KiB");return false};composing=value;return true}
            override fun finishComposingText():Boolean{if(composing.isNotEmpty()){val text=composing;composing="";commit(text)};return true}
            override fun deleteSurroundingText(beforeLength:Int,afterLength:Int):Boolean{
                if(composing.isNotEmpty()){composing=composing.dropLast(beforeLength.coerceIn(0,composing.length));return true}
                onInput(ByteArray(beforeLength.coerceIn(0,32)){127}+"\u001b[3~".repeat(afterLength.coerceIn(0,32)).toByteArray());return true
            }
            override fun deleteSurroundingTextInCodePoints(beforeLength:Int,afterLength:Int)=deleteSurroundingText(beforeLength,afterLength)
            override fun getTextBeforeCursor(n:Int,flags:Int):CharSequence=""
            override fun getTextAfterCursor(n:Int,flags:Int):CharSequence=""
            override fun getSelectedText(flags:Int):CharSequence=""
            override fun performEditorAction(actionCode:Int):Boolean{finishComposingText();commitText("\r",1);return true}
            override fun sendKeyEvent(event:KeyEvent):Boolean{if(event.action==KeyEvent.ACTION_DOWN){if(event.keyCode==KeyEvent.KEYCODE_ENTER)finishComposingText();return onKeyDown(event.keyCode,event)};return true}
        }
    }
    private fun commit(text:String){
        if(text.isEmpty())return
        var value=text
        if(controlLatched){val first=value[0];value=when(first){'?'->"\u007f";' '->"\u0000";else->if(first.code in 64..127||first.code in 97..122)(first.code and 31).toChar().toString()else first.toString()}+value.drop(1)}
        if(altLatched)value="\u001b$value"
        onInput(value.toByteArray(Charsets.UTF_8));consumeModifiers()
    }
    override fun onKeyDown(keyCode:Int,event:KeyEvent):Boolean{
        if(!inputEnabled)return super.onKeyDown(keyCode,event)
        if(keyCode in listOf(KeyEvent.KEYCODE_SHIFT_LEFT,KeyEvent.KEYCODE_SHIFT_RIGHT,KeyEvent.KEYCODE_CTRL_LEFT,KeyEvent.KEYCODE_CTRL_RIGHT,KeyEvent.KEYCODE_ALT_LEFT,KeyEvent.KEYCODE_ALT_RIGHT))return true
        val control=controlLatched||event.isCtrlPressed;val alt=altLatched||event.isAltPressed
        val modifier=1+(if(event.isShiftPressed)1 else 0)+(if(alt)2 else 0)+(if(control)4 else 0)
        fun cursor(last:Char)=if(modifier>1)"\u001b[1;${modifier}$last"else if(frame?.applicationCursor==true)"\u001bO$last"else "\u001b[$last"
        val value=when(keyCode){
            KeyEvent.KEYCODE_ENTER,KeyEvent.KEYCODE_NUMPAD_ENTER->"\r"
            KeyEvent.KEYCODE_DEL->if(alt)"\u001b\u007f"else "\u007f"
            KeyEvent.KEYCODE_FORWARD_DEL->"\u001b[3~"
            KeyEvent.KEYCODE_ESCAPE->"\u001b"
            KeyEvent.KEYCODE_TAB->if(event.isShiftPressed)"\u001b[Z"else "\t"
            KeyEvent.KEYCODE_DPAD_UP->cursor('A');KeyEvent.KEYCODE_DPAD_DOWN->cursor('B');KeyEvent.KEYCODE_DPAD_RIGHT->cursor('C');KeyEvent.KEYCODE_DPAD_LEFT->cursor('D')
            KeyEvent.KEYCODE_MOVE_HOME->cursor('H');KeyEvent.KEYCODE_MOVE_END->cursor('F')
            KeyEvent.KEYCODE_PAGE_UP->"\u001b[5~";KeyEvent.KEYCODE_PAGE_DOWN->"\u001b[6~";KeyEvent.KEYCODE_INSERT->"\u001b[2~"
            in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F4->"\u001bO${('P'.code+keyCode-KeyEvent.KEYCODE_F1).toChar()}"
            in KeyEvent.KEYCODE_F5..KeyEvent.KEYCODE_F12->"\u001b[${listOf(15,17,18,19,20,21,23,24)[keyCode-KeyEvent.KEYCODE_F5]}~"
            else->{
                var code=event.getUnicodeChar(event.metaState and KeyEvent.META_CTRL_MASK.inv())
                if(code==0)return super.onKeyDown(keyCode,event)
                if(code and KeyCharacterMap.COMBINING_ACCENT!=0){pendingAccent=code and KeyCharacterMap.COMBINING_ACCENT_MASK;return true}
                var prefix=""
                if(pendingAccent!=0){val combined=KeyCharacterMap.getDeadChar(pendingAccent,code);if(combined==0)prefix=String(Character.toChars(pendingAccent))else code=combined;pendingAccent=0}
                var text=prefix+String(Character.toChars(code))
                if(control)text=when(text){"?"->"\u007f";" "->"\u0000";else->(text[0].code and 31).toChar().toString()}
                if(alt)"\u001b$text"else text
            }
        }
        onInput(value.toByteArray(Charsets.UTF_8));consumeModifiers();return true
    }
    private fun consumeModifiers(){controlLatched=false;altLatched=false;onModifiersConsumed()}
    // Some Android 9 input methods deliver composed text as an ACTION_MULTIPLE event.
    @Suppress("DEPRECATION")
    override fun onKeyMultiple(keyCode: Int, repeatCount: Int, event: KeyEvent): Boolean {
        val text = event.characters
        if (text != null && inputEnabled) {
            commit(text)
            return true
        }
        return super.onKeyMultiple(keyCode, repeatCount, event)
    }
    override fun onSizeChanged(w:Int,h:Int,oldw:Int,oldh:Int){super.onSizeChanged(w,h,oldw,oldh);if(w>0&&h>0)onResize(TerminalRect((w/cellWidth).toInt().coerceIn(1,256),(h/lineHeight).toInt().coerceIn(1,128),w.coerceAtMost(16384),h.coerceAtMost(16384)))}
    override fun onDraw(canvas:Canvas){
        super.onDraw(canvas)
        val value = frame ?: return
        canvas.withTranslation(-panX, -panY) {
            for ((row, cells) in value.cells.withIndex()) for ((col, cell) in cells.withIndex()) {
                paint.color = if (cell.attributes and 16 != 0) color(cell.foreground, true)
                    else color(cell.background, false)
                paint.alpha = 255
                drawRect(col * cellWidth, row * lineHeight, (col + 1) * cellWidth, (row + 1) * lineHeight, paint)
            }
            for ((row, cells) in value.cells.withIndex()) for ((col, cell) in cells.withIndex()) {
                if (cell.width == 0 || cell.text.isEmpty()) continue
                paint.color = if (cell.attributes and 16 != 0) color(cell.background, false)
                    else color(cell.foreground, true)
                paint.alpha = if (cell.attributes and 2 != 0) 150 else 255
                val bold = if (cell.attributes and 1 != 0) 1 else 0
                val italic = if (cell.attributes and 4 != 0) 2 else 0
                paint.typeface = fonts[bold + italic]
                val left = col * cellWidth
                val top = row * lineHeight
                val bottom = top + lineHeight
                val cellRight = (col + cell.width) * cellWidth
                withClip(left, top, minOf(value.cols * cellWidth, cellRight), bottom) {
                    drawText(cell.text, left, top + baseline, paint)
                    if (cell.attributes and 8 != 0) {
                        paint.strokeWidth = 1f
                        drawLine(left, bottom - 1, cellRight, bottom - 1, paint)
                    }
                }
            }
            if (!value.hideCursor && value.cursorRow in 0 until value.rows && value.cursorCol in 0 until value.cols) {
                paint.color = DEFAULT_FOREGROUND
                paint.alpha = 85
                val left = value.cursorCol * cellWidth
                val top = value.cursorRow * lineHeight
                drawRect(left, top, (value.cursorCol + 1) * cellWidth, (value.cursorRow + 1) * lineHeight, paint)
                paint.alpha = 255
            }
        }
    }
    override fun onTouchEvent(event:MotionEvent):Boolean{
        when(event.actionMasked){
            MotionEvent.ACTION_DOWN->{downY=event.y;lastY=event.y;downX=event.x;lastX=event.x;dragged=false;horizontalDrag=false;parent?.requestDisallowInterceptTouchEvent(true);return true}
            MotionEvent.ACTION_MOVE->{
                if(!dragged&&max(abs(event.y-downY),abs(event.x-downX))>ViewConfiguration.get(context).scaledTouchSlop){dragged=true;horizontalDrag=abs(event.x-downX)>abs(event.y-downY)}
                if(dragged){
                    if(horizontalDrag){panX=(panX+lastX-event.x).coerceIn(0f,maxOf(0f,(frame?.cols?:0)*cellWidth-width));lastX=event.x;invalidate()}
                    else{val maximum=maxOf(0f,(frame?.rows?:0)*lineHeight-height);val desired=panY+lastY-event.y
                        if(maximum>0){panY=desired.coerceIn(0f,maximum);lastY=event.y;invalidate()}
                        else{val rows=((event.y-lastY)/lineHeight).toInt();if(rows!=0){onScroll(rows);lastY+=rows*lineHeight}}
                    }
                };return true}
            MotionEvent.ACTION_UP->{parent?.requestDisallowInterceptTouchEvent(false);if(!dragged)performClick();return true}
            MotionEvent.ACTION_CANCEL->{parent?.requestDisallowInterceptTouchEvent(false);return true}
        }
        return super.onTouchEvent(event)
    }
    override fun performClick():Boolean{super.performClick();showKeyboard();return true}
    override fun onGenericMotionEvent(event:MotionEvent):Boolean{if(event.action==MotionEvent.ACTION_SCROLL){onScroll((event.getAxisValue(MotionEvent.AXIS_VSCROLL)*3).roundToInt());return true};return super.onGenericMotionEvent(event)}
    override fun onInitializeAccessibilityNodeInfo(info:AccessibilityNodeInfo){super.onInitializeAccessibilityNodeInfo(info);info.className="android.widget.EditText";info.isEditable=inputEnabled;info.isMultiLine=true;info.text=contentDescription}
    companion object {
        private const val DEFAULT_FOREGROUND=0xffe4e8ef.toInt();private const val DEFAULT_BACKGROUND=0xff10131b.toInt()
        private val palette=intArrayOf(0x000000,0xcd0000,0x00cd00,0xcdcd00,0x0000ee,0xcd00cd,0x00cdcd,0xe5e5e5,0x7f7f7f,0xff0000,0x00ff00,0xffff00,0x5c5cff,0xff00ff,0x00ffff,0xffffff)
        private fun color(value:Int,foreground:Boolean):Int{
            val rgb=when{value<0->return if(foreground)DEFAULT_FOREGROUND else DEFAULT_BACKGROUND;value<16->palette[value];value<232->{val n=value-16;val levels=intArrayOf(0,95,135,175,215,255);(levels[n/36] shl 16)or(levels[(n/6)%6] shl 8)or levels[n%6]};value<256->{val level=8+(value-232)*10;(level shl 16)or(level shl 8)or level};else->value and 0xffffff}
            return 0xff000000.toInt() or rgb
        }
    }
}

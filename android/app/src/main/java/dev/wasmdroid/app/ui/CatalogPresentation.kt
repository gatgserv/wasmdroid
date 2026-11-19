package dev.wasmdroid.app.ui

/** An opaque accepted-source incarnation plus the exact reviewed entry bytes.
 * Equal pixels/IDs never make an old source selection current again. */
internal data class CatalogSelection(val source:String,val entry:String) {
    override fun toString()="CatalogSelection(private)"
}

internal data class CatalogPresentationFrame(
    val source:String?,val entries:List<String>,val unavailable:Boolean,
) {
    fun select(entry:String):CatalogSelection? = source?.takeIf{!unavailable&&entry in entries}
        ?.let{CatalogSelection(it,entry)}
    fun accepts(selection:CatalogSelection):Boolean = !unavailable&&source!=null&&
        source==selection.source&&selection.entry in entries
}

/** Results retain their selection independently of the cache's content key. */
internal data class CatalogPublication<T>(val selection:CatalogSelection,val value:T) {
    fun visible(frame:CatalogPresentationFrame):T?=value.takeIf{frame.accepts(selection)}
}

package dev.wasmdroid.app.ui

import java.security.MessageDigest
import java.util.Base64

internal enum class CatalogImageRole { Icon, Screenshot }
internal enum class CatalogImageFormat(val wire:String,val mime:String) {
    Png("png","image/png"), Jpeg("jpeg","image/jpeg")
}
internal data class CatalogImageProvenance(val license:String,val attribution:String,val report:String)
internal data class CatalogDisplayImage(
    val role:CatalogImageRole,val format:CatalogImageFormat,val sizeBytes:Int,
    val digest:String,val width:Int,val height:Int,val encoded:String,val alt:String,
    val provenance:CatalogImageProvenance,
) {
    override fun toString()="CatalogDisplayImage(private)"
}
internal data class CatalogDisplayMedia(val icon:CatalogDisplayImage,val screenshots:List<CatalogDisplayImage>)

/** Display validation only. Full-catalog native validation/review supplies trust;
 * these bytes never become an image URL, file or runtime permission. */
internal object CatalogImagePayload {
    const val MAX_MEDIA_BYTES=48*1024
    fun bytes(image:CatalogDisplayImage):ByteArray {
        val maximum=if(image.role==CatalogImageRole.Icon)8*1024 else 20*1024
        require(image.sizeBytes in 1..maximum)
        require(image.width>0&&image.height>0)
        if(image.role==CatalogImageRole.Icon)require(image.width<=256&&image.height<=256)
        else require(maxOf(image.width,image.height)<=1280&&minOf(image.width,image.height)<=720&&
            image.width.toLong()*image.height<=720L*1280L)
        require(text(image.alt,240)&&text(image.provenance.license,128)&&text(image.provenance.attribution,256))
        require(report(image.provenance.report))
        require(image.digest.matches(Regex("sha256:[0-9a-f]{64}")))
        require(image.encoded.length<=((maximum+2)/3)*4)
        val bytes=Base64.getDecoder().decode(image.encoded)
        require(bytes.size==image.sizeBytes&&Base64.getEncoder().encodeToString(bytes)==image.encoded)
        val digest=MessageDigest.getInstance("SHA-256").digest(bytes).joinToString(""){
            (it.toInt() and 255).toString(16).padStart(2,'0')
        }
        require(image.digest=="sha256:$digest")
        when(image.format){
            CatalogImageFormat.Png->require(bytes.size>=8&&bytes.copyOfRange(0,8).contentEquals(
                byteArrayOf(0x89.toByte(),0x50,0x4e,0x47,0x0d,0x0a,0x1a,0x0a)))
            CatalogImageFormat.Jpeg->require(bytes.size>=4&&bytes[0]==0xff.toByte()&&bytes[1]==0xd8.toByte()&&
                bytes[bytes.lastIndex-1]==0xff.toByte()&&bytes.last()==0xd9.toByte())
        }
        return bytes
    }
    fun validate(media:CatalogDisplayMedia){
        require(media.icon.role==CatalogImageRole.Icon&&media.screenshots.size in 1..2)
        var total=bytes(media.icon).size
        media.screenshots.forEach{image->
            require(image.role==CatalogImageRole.Screenshot)
            total+=bytes(image).size
        }
        require(total<=MAX_MEDIA_BYTES)
    }
    private fun text(value:String,maximum:Int)=value.isNotBlank()&&value.toByteArray(Charsets.UTF_8).size<=maximum&&
        value.none{it.isISOControl()}
    private fun report(value:String)=text(value,512)&&value.startsWith("docs/")&&value.endsWith(".md")&&
        value.split('/').all{it.isNotEmpty()&&it!="."&&it!=".."&&it.all{c->c in 'a'..'z'||c in 'A'..'Z'||c in '0'..'9'||c in "._-"}}
}

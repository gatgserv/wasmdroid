package dev.wasmdroid.app.ui

import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.common.BitMatrix
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import dev.wasmdroid.app.engine.LanSelectionLedger
import java.net.URI

/** Pure local encoding. Neither DNS nor a QR generation service is contacted. */
internal fun lanQrMatrix(url:String):BitMatrix {
    require(url.toByteArray(Charsets.UTF_8).size<=8192){"LAN address exceeds QR input budget"}
    val uri=URI(url)
    require(uri.scheme=="http"&&uri.rawUserInfo==null&&uri.port in 1..65535&&
        uri.host!=null&&LanSelectionLedger.privateIpv4(uri.host)==uri.host&&
        uri.rawAuthority=="${uri.host}:${uri.port}"){"QR target is not an actual private LAN address"}
    return QRCodeWriter().encode(url,BarcodeFormat.QR_CODE,512,512,mapOf(
        EncodeHintType.CHARACTER_SET to "UTF-8",EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M,
        EncodeHintType.MARGIN to 4))
}

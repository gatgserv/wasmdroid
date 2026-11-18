package dev.wasmdroid.app

import com.google.zxing.BinaryBitmap
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.WriterException
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import dev.wasmdroid.app.ui.ActiveLanAddresses
import dev.wasmdroid.app.ui.lanQrMatrix
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/** Pure local encode/decode; no Bitmap, camera, Wi-Fi or URL request. */
class LanQrEncoderTest {
    @Test fun plainAddressesRequireAnActiveGrantAndDoNotDependOnQrEncoding() {
        val ipv4 = "http://10.0.2.16:49173/" + "x".repeat(4000)
        val ipv6 = "http://[fd00::16]:49173/"
        val frame = JSONObject().put("phase", "active").put("grant_epoch", 2)
            .put("url", ipv4).put("ipv6_url", ipv6)
        assertThrows(WriterException::class.java) { lanQrMatrix(ipv4) }
        assertEquals(ActiveLanAddresses(ipv4, ipv6), ActiveLanAddresses.fromFrame(frame))
        assertNull(ActiveLanAddresses.fromFrame(JSONObject(frame.toString()).put("phase", "stopping")))
        assertNull(ActiveLanAddresses.fromFrame(JSONObject(frame.toString()).put("grant_epoch", 0)))
        frame.put("url", JSONObject.NULL)
        assertEquals(ActiveLanAddresses(null, ipv6), ActiveLanAddresses.fromFrame(frame))
        frame.remove("ipv6_url")
        assertNull(ActiveLanAddresses.fromFrame(frame))
    }
    @Test fun realQrDecodesToExactlyTheCurrentPrivateAddressIncludingPathQueryAndFragment() {
        val address="http://192.168.1.24:49173/notes/index.html?view=all#first"
        val matrix=lanQrMatrix(address)
        val pixels=IntArray(matrix.width*matrix.height){index->if(matrix[index%matrix.width,index/matrix.width])0xff000000.toInt() else 0xffffffff.toInt()}
        val binary=BinaryBitmap(HybridBinarizer(RGBLuminanceSource(matrix.width,matrix.height,pixels)))
        assertEquals(address,QRCodeReader().decode(binary).text)
    }
    @Test fun qrRejectsForeignAuthorityAndNeverTruncatesAnAddressToFit() {
        for(address in listOf("https://192.168.1.24:8080/","http://127.0.0.1:8080/",
            "http://example.com:8080/","http://10.0.0.1:8080@example.com/","http://10.0.0.1:0/")){
            assertThrows(IllegalArgumentException::class.java){lanQrMatrix(address)}
        }
        assertThrows(IllegalArgumentException::class.java){lanQrMatrix("http://10.0.0.1:8080/"+"x".repeat(8192))}
        assertThrows(WriterException::class.java){lanQrMatrix("http://10.0.0.1:8080/"+"x".repeat(4000))}
    }
}

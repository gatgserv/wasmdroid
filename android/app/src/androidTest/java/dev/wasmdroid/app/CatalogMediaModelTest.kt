package dev.wasmdroid.app

import dev.wasmdroid.app.ui.*
import java.security.MessageDigest
import java.util.Base64
import org.junit.Assert.*
import org.junit.Test

/** Pure byte/metadata checks. These do not execute Android BitmapFactory or
 * establish screenshot provenance, image decoder behavior or catalog trust. */
class CatalogMediaModelTest {
    private fun fixture(role:CatalogImageRole=CatalogImageRole.Icon)=CatalogDisplayImage(role,CatalogImageFormat.Png,68,
        "sha256:43739c566e26fd7cb88f69d3864ea34740372f5ee99acac169e090beffbce5c6",1,1,
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=",
        "Synthetic test pixel",CatalogImageProvenance("LicenseRef-TestFixture","First-party contract fixture",
            "docs/verification/media-fixture-only.md"))
    private fun reject(action:()->Unit){try{action();fail("Invalid display input was accepted")}catch(_:IllegalArgumentException){}}

    @Test fun payloadRetainsExactBytesDigestAndRoleBounds(){
        val image=fixture();val bytes=CatalogImagePayload.bytes(image)
        assertEquals(68,bytes.size)
        assertEquals(image.encoded,Base64.getEncoder().encodeToString(bytes))
        assertEquals(image.digest,"sha256:"+MessageDigest.getInstance("SHA-256").digest(bytes).joinToString(""){
            (it.toInt() and 255).toString(16).padStart(2,'0')})
        CatalogImagePayload.validate(CatalogDisplayMedia(image,listOf(fixture(CatalogImageRole.Screenshot))))
    }
    @Test fun tamperedBytesDigestSizeOrNoncanonicalEncodingCannotBecomeDisplayPayload(){
        val image=fixture()
        reject{CatalogImagePayload.bytes(image.copy(digest="sha256:"+"0".repeat(64)))}
        reject{CatalogImagePayload.bytes(image.copy(sizeBytes=67))}
        reject{CatalogImagePayload.bytes(image.copy(encoded=image.encoded+"\n"))}
        reject{CatalogImagePayload.bytes(image.copy(encoded=image.encoded.trimEnd('=')))}
        reject{CatalogImagePayload.bytes(image.copy(format=CatalogImageFormat.Jpeg))}
    }
    @Test fun iconAndScreenshotDimensionsAndEncodedBudgetsAreIndependent(){
        val image=fixture()
        reject{CatalogImagePayload.bytes(image.copy(width=257))}
        reject{CatalogImagePayload.bytes(image.copy(height=0))}
        reject{CatalogImagePayload.bytes(image.copy(sizeBytes=8193))}
        val shot=fixture(CatalogImageRole.Screenshot)
        reject{CatalogImagePayload.bytes(shot.copy(width=1281))}
        reject{CatalogImagePayload.bytes(shot.copy(width=721,height=721))}
        reject{CatalogImagePayload.bytes(shot.copy(encoded="x".repeat(28_000)))}
    }
    @Test fun mediaRequiresOneIconAndBoundedScreenshots(){
        val icon=fixture();val shot=fixture(CatalogImageRole.Screenshot)
        reject{CatalogImagePayload.validate(CatalogDisplayMedia(icon,emptyList()))}
        reject{CatalogImagePayload.validate(CatalogDisplayMedia(icon,listOf(shot,shot,shot)))}
        reject{CatalogImagePayload.validate(CatalogDisplayMedia(shot,listOf(shot)))}
        reject{CatalogImagePayload.validate(CatalogDisplayMedia(icon,listOf(icon)))}
        CatalogImagePayload.validate(CatalogDisplayMedia(icon,listOf(shot,shot)))
    }
    @Test fun altAndProvenanceAreBoundedInertTextAndNeverAnImageUrl(){
        val image=fixture()
        reject{CatalogImagePayload.bytes(image.copy(alt=""))}
        reject{CatalogImagePayload.bytes(image.copy(alt="a".repeat(241)))}
        reject{CatalogImagePayload.bytes(image.copy(alt="line\nbreak"))}
        reject{CatalogImagePayload.bytes(image.copy(provenance=image.provenance.copy(report="https://example.com/image.png")))}
        reject{CatalogImagePayload.bytes(image.copy(provenance=image.provenance.copy(report="docs/../private.md")))}
        reject{CatalogImagePayload.bytes(image.copy(provenance=image.provenance.copy(license="")))}
        assertFalse(image.toString().contains(image.encoded))
    }
}

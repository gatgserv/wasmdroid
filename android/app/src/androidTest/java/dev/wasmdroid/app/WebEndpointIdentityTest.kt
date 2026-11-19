package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.ui.WebTarget
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class WebEndpointIdentityTest{
    @Test fun presentationChangesDoNotCloseTheActualEndpointButExecutionChangesDo(){
        val actual=WebTarget("app","A long display name".repeat(10),"http://127.0.0.1:4000/","first-generation",8080,"/")
        assertTrue(actual.sameEndpoint(actual.copy(name="A short display name")))
        assertFalse(actual.sameEndpoint(actual.copy(id="replacement-app")))
        assertFalse(actual.sameEndpoint(actual.copy(generation="replacement-generation")))
        assertFalse(actual.sameEndpoint(actual.copy(url="http://127.0.0.1:4001/")))
        assertFalse(actual.sameEndpoint(actual.copy(instancePort=8081)))
        assertFalse(actual.sameEndpoint(actual.copy(path="/other")))
    }
}

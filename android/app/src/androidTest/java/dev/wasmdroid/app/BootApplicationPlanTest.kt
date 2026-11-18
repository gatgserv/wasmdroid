package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.engine.BootApplicationPlan
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class BootApplicationPlanTest {
    private fun record(id:String="app",state:String="exited",boot:Boolean?=true):JSONObject =
        JSONObject().put("id",id).put("version",2).put("driver","wasmer-local").put("state",state)
            .put("spec",JSONObject().put("endpoint_id","local").put("restart_policy","always").apply{
                if(boot!=null)put("start_on_boot",boot)
            })

    @Test fun bootRequiresSeparateExplicitOptInAndNeverSelectsManagedOrLiveWorkers() {
        val records=JSONArray().put(record("absent",boot=null)).put(record("off",boot=false))
            .put(record("managed")).put(record("running",state="running"))
            .put(record("dead",state="dead")).put(record("created",state="created"))
            .put(record("remote").apply{getJSONObject("spec").put("endpoint_id","remote")})
        val plan=BootApplicationPlan.candidates(records.toString(),setOf("managed"))
        assertEquals(listOf("created","dead"),plan.map{it.id})
        assertFalse(plan[0].replaceDead);assertTrue(plan[1].replaceDead)
    }

    @Test fun launchRechecksRevisionOptInMembershipAndOwnership() {
        val record=record();val candidate=BootApplicationPlan.candidates(JSONArray().put(record).toString(),emptySet()).single()
        assertTrue(BootApplicationPlan.valid(candidate,record.toString(),false,false))
        assertFalse(BootApplicationPlan.valid(candidate,record.toString(),true,false))
        assertFalse(BootApplicationPlan.valid(candidate,record.toString(),false,true))
        assertFalse(BootApplicationPlan.valid(candidate,JSONObject(record.toString()).put("version",3).toString(),false,false))
        record.getJSONObject("spec").put("start_on_boot",false)
        assertFalse(BootApplicationPlan.valid(candidate,record.toString(),false,false))
    }
}

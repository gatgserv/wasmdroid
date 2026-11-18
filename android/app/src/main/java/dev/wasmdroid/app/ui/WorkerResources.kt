package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import dev.wasmdroid.app.R
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import org.json.JSONObject
import java.util.Locale

internal data class WorkerSample(
    val generation:String, val residentBytes:Long?, val cpuNs:Long?, val monotonicNs:Long?,
) {
    fun startsCpuIntervalAfter(previous: WorkerSample?): Boolean =
        cpuNs != null && monotonicNs != null &&
            (previous == null || generation != previous.generation || previous.cpuNs == null || previous.monotonicNs == null)

    fun cpuPercent(previous:WorkerSample?):Double? {
        if(previous==null||generation!=previous.generation)return null
        val cpu=cpuNs?:return null;val oldCpu=previous.cpuNs?:return null
        val time=monotonicNs?:return null;val oldTime=previous.monotonicNs?:return null
        if(time<=oldTime||cpu<oldCpu)return null
        // One fully occupied CPU = 100%. Multiple worker threads may exceed 100%.
        return (cpu-oldCpu).toDouble()/(time-oldTime).toDouble()*100.0
    }
    companion object {
        fun parse(raw:String):WorkerSample {
            val json=JSONObject(raw)
            require(json.getString("scope")=="worker_process"){"Unknown resource measurement scope"}
            fun value(key:String):Long?=if(json.isNull(key))null else json.getLong(key).also{require(it>=0)}
            return WorkerSample(json.getString("generation"),value("memory_bytes"),value("cpu_time_ns"),value("sampled_at_monotonic_ns"))
        }
    }
}

@Composable internal fun WorkerResourceSummary(id:String,model:AppModel) {
    val lifecycle=LocalLifecycleOwner.current.lifecycle
    var sample by remember(id){mutableStateOf<WorkerSample?>(null)}
    var percent by remember(id){mutableStateOf<Double?>(null)}
    var measuringCpu by remember(id) { mutableStateOf(true) }
    LaunchedEffect(id,lifecycle,model){
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED){
            // A new visible interval always begins with two fresh samples.
            sample=null;percent=null;measuringCpu=true
            while(true){
                try{
                    val next=WorkerSample.parse(model.workerStats(id))
                    percent=next.cpuPercent(sample)
                    measuringCpu=next.startsCpuIntervalAfter(sample)
                    sample=next
                }catch(failure:Exception){
                    if(failure is CancellationException)throw failure
                    sample=null;percent=null;measuringCpu=false
                }
                delay(2_000)
            }
        }
    }
    val unavailable=stringResource(R.string.resources_unavailable)
    val memory=sample?.residentBytes?.let{String.format(Locale.ENGLISH,"%.1f MiB",it.toDouble()/1_048_576.0)}?:unavailable
    val cpu=percent?.let{String.format(Locale.ENGLISH,"%.1f%%",it)}
        ?: if (measuringCpu) stringResource(R.string.resources_measuring) else unavailable
    Column {
        Text(stringResource(R.string.resources_worker,memory,cpu),style=MaterialTheme.typography.bodySmall)
        Text(stringResource(R.string.resources_scope),style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

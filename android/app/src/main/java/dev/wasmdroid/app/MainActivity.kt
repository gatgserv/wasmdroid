package dev.wasmdroid.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import dev.wasmdroid.engine.engineInfo
import dev.wasmdroid.engine.runWasiProbe
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { MaterialTheme { SpikeScreen() } }
    }
}

@Composable
private fun SpikeScreen() {
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf("Verificăm motorul…") }
    var available by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        runCatching { withContext(Dispatchers.IO) { engineInfo() } }
            .onSuccess {
                available = it.runtimeProbeAvailable
                status = "Motor conectat · v${it.version} · API ${it.apiVersion}"
            }.onFailure { status = "Motorul nu a putut fi încărcat: ${it.message}" }
    }
    Surface(modifier = Modifier.fillMaxSize()) {
        Column(
            modifier = Modifier.safeDrawingPadding().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(20.dp),
        ) {
            Text("WasmDroid", style = MaterialTheme.typography.headlineLarge)
            Text("Laborator · etapa 0", style = MaterialTheme.typography.titleMedium)
            Text("Pregătim rularea aplicațiilor WebAssembly pe telefon. Catalogul va apărea după verificarea pachetelor pe Android.")
            Card { Text(status, modifier = Modifier.padding(20.dp)) }
            Button(enabled = available && !busy, onClick = {
                busy = true
                scope.launch {
                    status = runCatching { withContext(Dispatchers.IO) { runWasiProbe() } }
                        .fold(onSuccess = { "Proba WASI a reușit: ${it.trim()}" },
                            onFailure = { "Proba WASI a eșuat: ${it.message}" })
                    busy = false
                }
            }) { Text(if (busy) "Rulează…" else "Verifică rularea WASI") }
            if (!available) Text("Această versiune verifică legătura cu motorul. Proba de execuție necesită un build cu runtime Wasmer.")
        }
    }
}

package fr.apex.maint.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.*
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.BarcodeScannerOptions
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

@Composable fun ScannerScreen(vm: MaintViewModel, detected: (String) -> Unit) {
    val context = LocalContext.current
    var allowed by remember { mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) }
    var manual by rememberDraft(vm, "scanner:code", "")
    var error by remember { mutableStateOf<String?>(null) }
    var paused by remember { mutableStateOf(false) }
    val currentDetected by rememberUpdatedState(detected)
    val request = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        allowed = it; if (!it) error = "Accès à la caméra refusé. Utilisez la saisie manuelle."
    }
    Page {
        Text("Scanner un QR", style = MaterialTheme.typography.headlineSmall)
        Text("Présentez le code d’un équipement devant la caméra.")
        if (!allowed) Button({ request.launch(Manifest.permission.CAMERA) }) { Text("Autoriser la caméra") }
        if (allowed && !paused) CameraPreview(Modifier.fillMaxWidth().height(280.dp), { code ->
            paused = true; manual = code; currentDetected(code)
        }, { error = it })
        if (paused) OutlinedButton({ paused = false }) { Text("Reprendre la lecture") }
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        OutlinedTextField(manual, { manual = it }, label = { Text("Code équipement ou lien de l’instance") }, modifier = Modifier.fillMaxWidth())
        Button({ if (manual.isNotBlank()) currentDetected(manual) }, enabled = manual.isNotBlank(),
            modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text("Ouvrir la fiche") }
    }
}

@Composable private fun CameraPreview(modifier: Modifier, detected: (String) -> Unit, error: (String) -> Unit) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current
    val previewView = remember { PreviewView(context) }
    val currentDetected by rememberUpdatedState(detected)
    val currentError by rememberUpdatedState(error)
    AndroidView(factory = { previewView }, modifier = modifier)
    DisposableEffect(lifecycle, previewView) {
        val executor = Executors.newSingleThreadExecutor()
        val main = ContextCompat.getMainExecutor(context)
        val scanner = BarcodeScanning.getClient(BarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build())
        val active = AtomicBoolean(true)
        val delivered = AtomicBoolean(false)
        val providerFuture = ProcessCameraProvider.getInstance(context)
        var provider: ProcessCameraProvider? = null
        val preview = Preview.Builder().build().apply { setSurfaceProvider(previewView.surfaceProvider) }
        val analysis = ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
        analysis.setAnalyzer(executor, object : ImageAnalysis.Analyzer {
            @androidx.annotation.OptIn(ExperimentalGetImage::class)
            override fun analyze(frame: ImageProxy) {
                val image = frame.image
                if (!active.get() || image == null || delivered.get()) { frame.close(); return }
                val task = try { scanner.process(InputImage.fromMediaImage(image, frame.imageInfo.rotationDegrees)) }
                catch (_: Exception) {
                    frame.close()
                    main.execute { if (active.get()) currentError("Lecture du QR impossible. Utilisez la saisie manuelle.") }
                    return
                }
                task.addOnSuccessListener(main) { codes ->
                        codes.firstNotNullOfOrNull { it.rawValue }?.let { value ->
                            if (active.get() && delivered.compareAndSet(false, true)) currentDetected(value)
                        }
                    }
                    .addOnFailureListener(main) { if (active.get()) currentError("Lecture du QR impossible. Utilisez la saisie manuelle.") }
                    .addOnCompleteListener { frame.close() }
            }
        })
        providerFuture.addListener({
            if (active.get()) try {
                provider = providerFuture.get()
                provider?.bindToLifecycle(lifecycle, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
            } catch (_: Exception) { currentError("Caméra indisponible. Utilisez la saisie manuelle.") }
        }, main)
        onDispose {
            active.set(false); analysis.clearAnalyzer(); provider?.unbind(preview, analysis)
            scanner.close(); executor.shutdown()
        }
    }
}

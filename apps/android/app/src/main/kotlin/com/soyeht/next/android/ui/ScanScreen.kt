package com.soyeht.next.android.ui

import android.util.Size
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.unit.*
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.scan.QrDecoder
import com.soyeht.next.android.scan.RepeatFilter
import com.soyeht.next.android.theme.*
import java.util.concurrent.Executors

/**
 * The camera, looking for the QR code the computer shows. What it reads goes to the same parser as a
 * tapped link; a code the parser refuses is said in words and the camera keeps looking.
 */
@Composable
fun ColumnScope.ScanScreen(model: AppModel) {
    val roles = LocalRoles.current
    TopBar("Scan the code", "On the computer, choose “Pair phone”", onBack = model::stopScanning)
    Box(Modifier.weight(1f).fillMaxWidth().background(roles.well)) {
        CameraPreview(onText = model::scanned)
    }
    Text(
        model.scanProblem ?: "Point the camera at the QR code on the computer.",
        color = if (model.scanProblem != null) roles.ink else roles.secondary,
        fontSize = TypeScale.caption.sp,
        modifier = Modifier.fillMaxWidth().padding(PaneInterior.phoneInset.dp),
    )
}

@Composable
private fun CameraPreview(onText: (String) -> Unit) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current
    val latest by rememberUpdatedState(onText)
    val analysis = remember { Executors.newSingleThreadExecutor() }
    val preview = remember { PreviewView(context).apply { scaleType = PreviewView.ScaleType.FILL_CENTER } }
    DisposableEffect(lifecycle) {
        val decoder = QrDecoder()
        val repeats = RepeatFilter()
        val providerFuture = ProcessCameraProvider.getInstance(context)
        var provider: ProcessCameraProvider? = null
        providerFuture.addListener({
            val camera = providerFuture.get()
            provider = camera
            val frames = ImageAnalysis.Builder()
                .setResolutionSelector(
                    ResolutionSelector.Builder()
                        .setResolutionStrategy(
                            ResolutionStrategy(Size(WIDTH, HEIGHT), ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER),
                        ).build(),
                )
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .build()
            frames.setAnalyzer(analysis) { frame ->
                val text = read(decoder, frame)
                if (text != null && repeats.accept(text, System.currentTimeMillis())) {
                    ContextCompat.getMainExecutor(context).execute { latest(text) }
                }
            }
            val view = Preview.Builder().build().also { it.surfaceProvider = preview.surfaceProvider }
            camera.unbindAll()
            camera.bindToLifecycle(lifecycle, CameraSelector.DEFAULT_BACK_CAMERA, view, frames)
        }, ContextCompat.getMainExecutor(context))
        onDispose {
            provider?.unbindAll()
            analysis.shutdown()
        }
    }
    AndroidView({ preview }, Modifier.fillMaxSize())
}

/** The brightness plane of a frame, handed to the decoder; the frame is always closed. */
private fun read(decoder: QrDecoder, frame: ImageProxy): String? = frame.use {
    val plane = it.planes[0]
    val bytes = ByteArray(plane.rowStride * it.height)
    plane.buffer.get(bytes, 0, minOf(bytes.size, plane.buffer.remaining()))
    decoder.decode(bytes, it.width, it.height, plane.rowStride)
}

/** What the pair screen says when the camera cannot be used, and where else to pair. */
@Composable
fun CameraDenied() {
    val roles = LocalRoles.current
    Text(
        "The camera is off for Soyeht. Allow it in the phone’s settings to scan, or paste the link above.",
        color = roles.secondary,
        fontSize = TypeScale.caption.sp,
        modifier = Modifier
            .fillMaxWidth()
            .background(roles.well, RoundedCornerShape(Radius.control.dp))
            .padding(PaneInterior.cardPadding.dp),
    )
}

private const val WIDTH = 1280
private const val HEIGHT = 720

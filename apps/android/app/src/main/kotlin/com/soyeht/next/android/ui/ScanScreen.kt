package com.soyeht.next.android.ui

import android.Manifest
import android.content.pm.PackageManager
import android.util.Size
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.unit.*
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.scan.CameraProblem
import com.soyeht.next.android.scan.QrDecoder
import com.soyeht.next.android.scan.RepeatFilter
import com.soyeht.next.android.theme.*
import java.util.concurrent.ExecutionException
import java.util.concurrent.Executors

/**
 * The camera, looking for the QR code the computer shows. What it reads goes to the same parser as a
 * tapped link; a code the parser refuses is said in words and the camera keeps looking.
 */
@Composable
fun ColumnScope.ScanScreen(model: AppModel) {
    val roles = LocalRoles.current
    TopBar("Scan the code", PAIRING_ON_COMPUTER, onBack = model::stopScanning)
    Box(Modifier.weight(1f).fillMaxWidth().background(roles.well)) {
        CameraPreview(onText = model::scanned, onFailure = model::cameraFailed)
    }
    Text(
        model.scanProblem ?: "Point the camera at the QR code on the computer.",
        color = if (model.scanProblem != null) roles.ink else roles.secondary,
        fontSize = TypeScale.caption.sp,
        modifier = Modifier.fillMaxWidth().padding(PaneInterior.phoneInset.dp),
    )
}

@Composable
private fun CameraPreview(onText: (String) -> Unit, onFailure: () -> Unit) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current
    val latest by rememberUpdatedState(onText)
    val failed by rememberUpdatedState(onFailure)
    val preview = remember { PreviewView(context).apply { scaleType = PreviewView.ScaleType.FILL_CENTER } }
    DisposableEffect(lifecycle) {
        // Made here and ended with the effect: a restart of the effect starts a fresh one, never a dead one.
        val analysis = Executors.newSingleThreadExecutor()
        val decoder = QrDecoder()
        val repeats = RepeatFilter()
        val providerFuture = ProcessCameraProvider.getInstance(context)
        var provider: ProcessCameraProvider? = null
        providerFuture.addListener({
            // No camera on the device (an e-reader), the camera in use, or a camera that broke: the person is told
            // in words, never a crash. Everything the provider can refuse is raised here, so it is caught here.
            try {
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
            } catch (failure: ExecutionException) {
                failed()
            } catch (failure: IllegalArgumentException) {
                failed()
            } catch (failure: IllegalStateException) {
                failed()
            }
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

/**
 * What the Scan button does: starts the camera when it is allowed, else asks for it, now and not before. The
 * pair screen and the list of panes (to pair another computer) share it.
 */
@Composable
fun rememberStartScan(model: AppModel): () -> Unit {
    val context = LocalContext.current
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed ->
        if (allowed) model.startScanning() else model.cameraRefused()
    }
    return {
        val allowed = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED
        if (allowed) model.startScanning() else permission.launch(Manifest.permission.CAMERA)
    }
}

/** Whether this device has a camera at all: a tablet or an e-reader may have none, and then Scan is not offered. */
@Composable
fun rememberHasCamera(): Boolean {
    val context = LocalContext.current
    return remember { context.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY) }
}

/** What the screen says when the camera cannot be used, and where else to pair. */
@Composable
fun CameraNotice(problem: CameraProblem) {
    val roles = LocalRoles.current
    Text(
        problem.words,
        color = roles.secondary,
        fontSize = TypeScale.caption.sp,
        modifier = Modifier
            .fillMaxWidth()
            .background(roles.well, RoundedCornerShape(Radius.control.dp))
            .padding(PaneInterior.cardPadding.dp),
    )
}

/** The pill that takes the pairing link from the clipboard, for a device with no camera to scan with. */
@Composable
fun rememberPasteLink(onLink: (String) -> Unit): () -> Unit {
    val clipboard = LocalClipboardManager.current
    val latest by rememberUpdatedState(onLink)
    return { latest(clipboard.getText()?.text.orEmpty().trim()) }
}

private const val WIDTH = 1280
private const val HEIGHT = 720

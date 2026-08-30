package com.picoo.camera.ui

import android.content.Context
import android.hardware.display.DisplayManager
import android.graphics.Matrix
import android.graphics.SurfaceTexture
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.Surface
import android.view.TextureView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import kotlin.math.max
import java.util.concurrent.atomic.AtomicReference

internal data class PreviewTransform(
    val scaleX: Float,
    val scaleY: Float,
    val rotationDegrees: Float,
)

internal fun needsTextureViewMirror(
    mirrorLocal: Boolean,
    frontFacing: Boolean,
    platformMirrorCanBeDisabled: Boolean,
): Boolean = mirrorLocal xor (frontFacing && !platformMirrorCanBeDisabled)

/**
 * Camera2 TextureView transform adapted from Android's resizable Camera2 preview guidance.
 * It reverses TextureView's non-uniform fill, then applies one uniform center-crop scale.
 */
internal fun calculatePreviewTransform(
    viewWidth: Int,
    viewHeight: Int,
    bufferWidth: Int,
    bufferHeight: Int,
    sensorOrientationDegrees: Int,
    displayRotationDegrees: Int,
    frontFacing: Boolean,
): PreviewTransform {
    if (viewWidth <= 0 || viewHeight <= 0 || bufferWidth <= 0 || bufferHeight <= 0) {
        return PreviewTransform(1f, 1f, -displayRotationDegrees.toFloat())
    }

    val orientationSign = if (frontFacing) 1 else -1
    val relativeRotation =
        (sensorOrientationDegrees - displayRotationDegrees * orientationSign + 360) % 360
    val swapsAxes = relativeRotation % 180 != 0

    val initialScaleX: Float
    val initialScaleY: Float
    if (sensorOrientationDegrees == 0) {
        initialScaleX = viewWidth.toFloat() / if (swapsAxes) bufferWidth else bufferHeight
        initialScaleY = viewHeight.toFloat() / if (swapsAxes) bufferHeight else bufferWidth
    } else {
        initialScaleX = viewWidth.toFloat() / if (swapsAxes) bufferHeight else bufferWidth
        initialScaleY = viewHeight.toFloat() / if (swapsAxes) bufferWidth else bufferHeight
    }
    val centerCropScale = max(initialScaleX, initialScaleY)

    return if (swapsAxes) {
        PreviewTransform(
            scaleX = centerCropScale / initialScaleX,
            scaleY = centerCropScale / initialScaleY,
            rotationDegrees = -displayRotationDegrees.toFloat(),
        )
    } else {
        PreviewTransform(
            scaleX = viewHeight.toFloat() / viewWidth / initialScaleY * centerCropScale,
            scaleY = viewWidth.toFloat() / viewHeight / initialScaleX * centerCropScale,
            rotationDegrees = -displayRotationDegrees.toFloat(),
        )
    }
}

private fun TextureView.applyCameraTransform(
    bufferWidth: Int,
    bufferHeight: Int,
    sensorOrientationDegrees: Int,
    frontFacing: Boolean,
    mirrorLocal: Boolean,
) {
    surfaceTexture?.setDefaultBufferSize(bufferWidth, bufferHeight)
    val displayRotationDegrees = when (display?.rotation ?: Surface.ROTATION_0) {
        Surface.ROTATION_0 -> 0
        Surface.ROTATION_90 -> 90
        Surface.ROTATION_180 -> 180
        Surface.ROTATION_270 -> 270
        else -> 0
    }
    val transform = calculatePreviewTransform(
        viewWidth = width,
        viewHeight = height,
        bufferWidth = bufferWidth,
        bufferHeight = bufferHeight,
        sensorOrientationDegrees = sensorOrientationDegrees,
        displayRotationDegrees = displayRotationDegrees,
        frontFacing = frontFacing,
    )
    val centerX = width / 2f
    val centerY = height / 2f
    setTransform(
        Matrix().apply {
            setScale(transform.scaleX, transform.scaleY, centerX, centerY)
            postRotate(transform.rotationDegrees, centerX, centerY)
        },
    )
    // Mirror in display coordinates after rotation/crop; it never changes the encoded stream.
    pivotX = centerX
    pivotY = centerY
    val needsUiMirror = needsTextureViewMirror(
        mirrorLocal = mirrorLocal,
        frontFacing = frontFacing,
        platformMirrorCanBeDisabled = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU,
    )
    scaleX = if (needsUiMirror) -1f else 1f
}

private class CameraPreviewTextureView(context: Context) :
    TextureView(context),
    DisplayManager.DisplayListener {
    private val displayManager = context.applicationContext.getSystemService(DisplayManager::class.java)
    private val mainHandler = Handler(Looper.getMainLooper())
    private var listenerRegistered = false
    private var bufferWidth = 1280
    private var bufferHeight = 720
    private var sensorOrientationDegrees = 90
    private var frontFacing = false
    private var mirrorLocal = false

    var onDisplayConfigurationChanged: () -> Unit = {}

    fun updateTransform(
        bufferWidth: Int,
        bufferHeight: Int,
        sensorOrientationDegrees: Int,
        frontFacing: Boolean,
        mirrorLocal: Boolean,
    ) {
        this.bufferWidth = bufferWidth
        this.bufferHeight = bufferHeight
        this.sensorOrientationDegrees = sensorOrientationDegrees
        this.frontFacing = frontFacing
        this.mirrorLocal = mirrorLocal
        applyCurrentTransform()
    }

    fun applyCurrentTransform() {
        applyCameraTransform(
            bufferWidth = bufferWidth,
            bufferHeight = bufferHeight,
            sensorOrientationDegrees = sensorOrientationDegrees,
            frontFacing = frontFacing,
            mirrorLocal = mirrorLocal,
        )
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        if (!listenerRegistered) {
            displayManager.registerDisplayListener(this, mainHandler)
            listenerRegistered = true
        }
    }

    override fun onDetachedFromWindow() {
        if (listenerRegistered) {
            displayManager.unregisterDisplayListener(this)
            listenerRegistered = false
        }
        super.onDetachedFromWindow()
    }

    override fun onDisplayAdded(displayId: Int) = Unit

    override fun onDisplayRemoved(displayId: Int) = Unit

    override fun onDisplayChanged(displayId: Int) {
        if (displayId != display?.displayId) return
        onDisplayConfigurationChanged()
        applyCurrentTransform()
    }
}

/**
 * Local camera preview. [mirrorLocal] flips the TextureView only — it does not
 * affect StreamConfig.mirrored sent to the receiver (REQ-PICOO-MEDIA-004).
 */
@Composable
fun CameraPreviewSurface(
    modifier: Modifier = Modifier,
    bufferWidth: Int,
    bufferHeight: Int,
    sensorOrientationDegrees: Int,
    frontFacing: Boolean,
    mirrorLocal: Boolean = true,
    onSurfaceAvailable: (SurfaceTexture) -> Unit,
    onSurfaceDestroyed: (SurfaceTexture) -> Unit,
    onDisplayChanged: () -> Unit,
) {
    val currentOnSurfaceAvailable = rememberUpdatedState(onSurfaceAvailable)
    val currentOnSurfaceDestroyed = rememberUpdatedState(onSurfaceDestroyed)
    val currentOnDisplayChanged = rememberUpdatedState(onDisplayChanged)
    val currentBufferWidth = rememberUpdatedState(bufferWidth)
    val currentBufferHeight = rememberUpdatedState(bufferHeight)
    val currentSensorOrientationDegrees = rememberUpdatedState(sensorOrientationDegrees)
    val currentFrontFacing = rememberUpdatedState(frontFacing)
    val currentMirrorLocal = rememberUpdatedState(mirrorLocal)
    val activeSurfaceTexture = remember { AtomicReference<SurfaceTexture?>(null) }

    DisposableEffect(Unit) {
        onDispose {
            activeSurfaceTexture.getAndSet(null)?.let(currentOnSurfaceDestroyed.value)
        }
    }

    AndroidView(
        modifier = modifier,
        factory = { context ->
            CameraPreviewTextureView(context).apply {
                onDisplayConfigurationChanged = { currentOnDisplayChanged.value() }
                updateTransform(
                    bufferWidth,
                    bufferHeight,
                    sensorOrientationDegrees,
                    frontFacing,
                    mirrorLocal,
                )
                surfaceTextureListener =
                    object : TextureView.SurfaceTextureListener {
                        override fun onSurfaceTextureAvailable(
                            surfaceTexture: SurfaceTexture,
                            width: Int,
                            height: Int,
                        ) {
                            activeSurfaceTexture.set(surfaceTexture)
                            updateTransform(
                                currentBufferWidth.value,
                                currentBufferHeight.value,
                                currentSensorOrientationDegrees.value,
                                currentFrontFacing.value,
                                currentMirrorLocal.value,
                            )
                            currentOnSurfaceAvailable.value(surfaceTexture)
                        }

                        override fun onSurfaceTextureSizeChanged(
                            surfaceTexture: SurfaceTexture,
                            width: Int,
                            height: Int,
                        ) {
                            applyCurrentTransform()
                        }

                        override fun onSurfaceTextureDestroyed(
                            surfaceTexture: SurfaceTexture,
                        ): Boolean {
                            if (activeSurfaceTexture.compareAndSet(surfaceTexture, null)) {
                                currentOnSurfaceDestroyed.value(surfaceTexture)
                            }
                            return true
                        }

                        override fun onSurfaceTextureUpdated(surfaceTexture: SurfaceTexture) = Unit
                    }
            }
        },
        update = { view ->
            view.onDisplayConfigurationChanged = { currentOnDisplayChanged.value() }
            view.updateTransform(
                bufferWidth,
                bufferHeight,
                sensorOrientationDegrees,
                frontFacing,
                mirrorLocal,
            )
        },
    )
}

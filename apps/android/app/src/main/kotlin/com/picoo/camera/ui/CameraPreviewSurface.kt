package com.picoo.camera.ui

import android.graphics.SurfaceTexture
import android.view.Surface
import android.view.TextureView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView

/**
 * Local camera preview. [mirrorLocal] flips the TextureView only — it does not
 * affect StreamConfig.mirrored sent to the receiver (REQ-PICOO-MEDIA-004).
 */
@Composable
fun CameraPreviewSurface(
    modifier: Modifier = Modifier,
    mirrorLocal: Boolean = true,
    onSurfaceAvailable: (Surface) -> Unit,
    onSurfaceDestroyed: () -> Unit,
) {
    DisposableEffect(Unit) {
        onDispose { onSurfaceDestroyed() }
    }

    AndroidView(
        modifier = modifier,
        factory = { context ->
            TextureView(context).apply {
                surfaceTextureListener =
                    object : TextureView.SurfaceTextureListener {
                        override fun onSurfaceTextureAvailable(
                            surfaceTexture: SurfaceTexture,
                            width: Int,
                            height: Int,
                        ) {
                            surfaceTexture.setDefaultBufferSize(1280, 720)
                            onSurfaceAvailable(Surface(surfaceTexture))
                        }

                        override fun onSurfaceTextureSizeChanged(
                            surfaceTexture: SurfaceTexture,
                            width: Int,
                            height: Int,
                        ) = Unit

                        override fun onSurfaceTextureDestroyed(surfaceTexture: SurfaceTexture): Boolean {
                            onSurfaceDestroyed()
                            return true
                        }

                        override fun onSurfaceTextureUpdated(surfaceTexture: SurfaceTexture) = Unit
                    }
            }
        },
        update = { view ->
            // Local preview mirror only (independent of remote StreamConfig.mirrored).
            view.scaleX = if (mirrorLocal) -1f else 1f
        },
    )
}

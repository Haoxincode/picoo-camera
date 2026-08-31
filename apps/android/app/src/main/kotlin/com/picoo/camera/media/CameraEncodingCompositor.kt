package com.picoo.camera.media

import android.graphics.SurfaceTexture
import android.opengl.EGL14
import android.opengl.EGLConfig
import android.opengl.EGLContext
import android.opengl.EGLDisplay
import android.opengl.EGLExt
import android.opengl.EGLSurface
import android.opengl.GLES11Ext
import android.opengl.GLES20
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.util.Size
import android.view.Surface
import java.io.Closeable
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * Camera2 OES texture → fixed landscape MediaCodec InputSurface compositor.
 *
 * All EGL/GL and SurfaceTexture calls are serialized on one dedicated thread;
 * Camera2 only sees [cameraInputSurface], while MediaCodec owns the output
 * [Surface] supplied to [create].
 */
internal class CameraEncodingCompositor private constructor(
    private val encoderSurface: Surface,
    private val cameraBufferSize: Size,
    private val outputSize: Size,
    initialRotationDegrees: Int,
    private val onError: (String) -> Unit,
) : Closeable {
    private val thread = HandlerThread("picoo-compositor").apply { start() }
    private val handler = Handler(thread.looper)
    private val released = AtomicBoolean(false)
    private val runtimeErrorReported = AtomicBoolean(false)

    @Volatile
    private var encodingMatrix = EncodingTransform.outputToCameraTextureMatrix(
        cameraBufferWidth = cameraBufferSize.width,
        cameraBufferHeight = cameraBufferSize.height,
        outputWidth = outputSize.width,
        outputHeight = outputSize.height,
        clockwiseRotationDegrees = initialRotationDegrees,
    )

    @Volatile
    private var inputSurface: Surface? = null

    private var inputSurfaceTexture: SurfaceTexture? = null
    private var eglDisplay: EGLDisplay = EGL14.EGL_NO_DISPLAY
    private var eglContext: EGLContext = EGL14.EGL_NO_CONTEXT
    private var eglSurface: EGLSurface = EGL14.EGL_NO_SURFACE
    private var textureId = 0
    private var programId = 0
    private var positionLocation = -1
    private var textureCoordinateLocation = -1
    private var textureMatrixLocation = -1
    private var textureSamplerLocation = -1
    private val producerTextureMatrix = FloatArray(MATRIX_SIZE)
    private val positionBuffer = floatBufferOf(
        -1f, -1f,
        1f, -1f,
        -1f, 1f,
        1f, 1f,
    )
    private val textureCoordinateBuffer = floatBufferOf(
        0f, 0f,
        1f, 0f,
        0f, 1f,
        1f, 1f,
    )

    val cameraInputSurface: Surface
        get() = checkNotNull(inputSurface) { "compositor is not initialized" }

    fun updateRotation(rotationDegrees: Int) {
        encodingMatrix = EncodingTransform.outputToCameraTextureMatrix(
            cameraBufferWidth = cameraBufferSize.width,
            cameraBufferHeight = cameraBufferSize.height,
            outputWidth = outputSize.width,
            outputHeight = outputSize.height,
            clockwiseRotationDegrees = rotationDegrees,
        )
    }

    override fun close() {
        if (!released.compareAndSet(false, true)) return
        val closed = CountDownLatch(1)
        val closeAction = {
            runCatching { releaseOnRenderThread() }
            closed.countDown()
        }
        if (Looper.myLooper() == handler.looper) {
            closeAction()
        } else if (!handler.post(closeAction)) {
            closed.countDown()
        }
        closed.await(RELEASE_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        thread.quitSafely()
    }

    private fun initialize(): Result<Unit> {
        val initialized = CountDownLatch(1)
        val error = AtomicReference<Throwable?>(null)
        if (!handler.post {
                runCatching { initializeOnRenderThread() }
                    .onFailure {
                        error.set(it)
                        runCatching { releaseOnRenderThread() }
                    }
                initialized.countDown()
            }
        ) {
            close()
            return Result.failure(IllegalStateException("compositor thread rejected initialization"))
        }
        if (!initialized.await(INIT_TIMEOUT_SECONDS, TimeUnit.SECONDS)) {
            close()
            return Result.failure(IllegalStateException("compositor initialization timed out"))
        }
        val failure = error.get()
        if (failure != null) {
            close()
            return Result.failure(failure)
        }
        return Result.success(Unit)
    }

    private fun initializeOnRenderThread() {
        eglDisplay = EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY)
        check(eglDisplay != EGL14.EGL_NO_DISPLAY) { "eglGetDisplay failed" }
        val versions = IntArray(2)
        check(EGL14.eglInitialize(eglDisplay, versions, 0, versions, 1)) {
            "eglInitialize failed: 0x${EGL14.eglGetError().toString(16)}"
        }
        val config = chooseEglConfig()
        eglContext = EGL14.eglCreateContext(
            eglDisplay,
            config,
            EGL14.EGL_NO_CONTEXT,
            intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION, 2, EGL14.EGL_NONE),
            0,
        )
        check(eglContext != EGL14.EGL_NO_CONTEXT) {
            "eglCreateContext failed: 0x${EGL14.eglGetError().toString(16)}"
        }
        eglSurface = EGL14.eglCreateWindowSurface(
            eglDisplay,
            config,
            encoderSurface,
            intArrayOf(EGL14.EGL_NONE),
            0,
        )
        check(eglSurface != EGL14.EGL_NO_SURFACE) {
            "eglCreateWindowSurface failed: 0x${EGL14.eglGetError().toString(16)}"
        }
        makeEncoderSurfaceCurrent()

        programId = createProgram(VERTEX_SHADER, FRAGMENT_SHADER)
        positionLocation = GLES20.glGetAttribLocation(programId, "aPosition")
        textureCoordinateLocation = GLES20.glGetAttribLocation(programId, "aTextureCoordinate")
        textureMatrixLocation = GLES20.glGetUniformLocation(programId, "uTextureMatrix")
        textureSamplerLocation = GLES20.glGetUniformLocation(programId, "uTexture")
        check(
            positionLocation >= 0 &&
                textureCoordinateLocation >= 0 &&
                textureMatrixLocation >= 0 &&
                textureSamplerLocation >= 0,
        ) {
            "compositor shader locations are incomplete"
        }

        val textures = IntArray(1)
        GLES20.glGenTextures(1, textures, 0)
        textureId = textures[0]
        check(textureId != 0) { "glGenTextures failed" }
        GLES20.glBindTexture(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, textureId)
        GLES20.glTexParameteri(
            GLES11Ext.GL_TEXTURE_EXTERNAL_OES,
            GLES20.GL_TEXTURE_MIN_FILTER,
            GLES20.GL_LINEAR,
        )
        GLES20.glTexParameteri(
            GLES11Ext.GL_TEXTURE_EXTERNAL_OES,
            GLES20.GL_TEXTURE_MAG_FILTER,
            GLES20.GL_LINEAR,
        )
        GLES20.glTexParameteri(
            GLES11Ext.GL_TEXTURE_EXTERNAL_OES,
            GLES20.GL_TEXTURE_WRAP_S,
            GLES20.GL_CLAMP_TO_EDGE,
        )
        GLES20.glTexParameteri(
            GLES11Ext.GL_TEXTURE_EXTERNAL_OES,
            GLES20.GL_TEXTURE_WRAP_T,
            GLES20.GL_CLAMP_TO_EDGE,
        )

        val surfaceTexture = SurfaceTexture(textureId).apply {
            setDefaultBufferSize(cameraBufferSize.width, cameraBufferSize.height)
            setOnFrameAvailableListener({ renderLatestFrame() }, handler)
        }
        inputSurfaceTexture = surfaceTexture
        inputSurface = Surface(surfaceTexture)
        throwOnGlError("initialize compositor")
    }

    private fun renderLatestFrame() {
        if (released.get()) return
        runCatching {
            val surfaceTexture = checkNotNull(inputSurfaceTexture)
            makeEncoderSurfaceCurrent()
            surfaceTexture.updateTexImage()
            surfaceTexture.getTransformMatrix(producerTextureMatrix)
            val textureMatrix = EncodingTransform.multiply(
                producerTextureMatrix,
                encodingMatrix,
            )

            GLES20.glViewport(0, 0, outputSize.width, outputSize.height)
            GLES20.glUseProgram(programId)
            GLES20.glActiveTexture(GLES20.GL_TEXTURE0)
            GLES20.glBindTexture(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, textureId)
            GLES20.glUniform1i(textureSamplerLocation, 0)
            positionBuffer.position(0)
            textureCoordinateBuffer.position(0)
            GLES20.glEnableVertexAttribArray(positionLocation)
            GLES20.glVertexAttribPointer(
                positionLocation,
                COORDINATES_PER_VERTEX,
                GLES20.GL_FLOAT,
                false,
                0,
                positionBuffer,
            )
            GLES20.glEnableVertexAttribArray(textureCoordinateLocation)
            GLES20.glVertexAttribPointer(
                textureCoordinateLocation,
                COORDINATES_PER_VERTEX,
                GLES20.GL_FLOAT,
                false,
                0,
                textureCoordinateBuffer,
            )
            GLES20.glUniformMatrix4fv(textureMatrixLocation, 1, false, textureMatrix, 0)
            GLES20.glDrawArrays(GLES20.GL_TRIANGLE_STRIP, 0, VERTEX_COUNT)
            EGLExt.eglPresentationTimeANDROID(
                eglDisplay,
                eglSurface,
                surfaceTexture.timestamp,
            )
            check(EGL14.eglSwapBuffers(eglDisplay, eglSurface)) {
                "eglSwapBuffers failed: 0x${EGL14.eglGetError().toString(16)}"
            }
            throwOnGlError("render camera frame")
        }.onFailure { error ->
            if (runtimeErrorReported.compareAndSet(false, true)) {
                onError("Encoding compositor failed: ${error.message ?: error.javaClass.simpleName}")
            }
        }
    }

    private fun chooseEglConfig(): EGLConfig {
        val configs = arrayOfNulls<EGLConfig>(1)
        val count = IntArray(1)
        val attributes = intArrayOf(
            EGL14.EGL_RED_SIZE, 8,
            EGL14.EGL_GREEN_SIZE, 8,
            EGL14.EGL_BLUE_SIZE, 8,
            EGL14.EGL_ALPHA_SIZE, 8,
            EGL14.EGL_RENDERABLE_TYPE, EGL14.EGL_OPENGL_ES2_BIT,
            EGL_RECORDABLE_ANDROID, 1,
            EGL14.EGL_NONE,
        )
        check(EGL14.eglChooseConfig(eglDisplay, attributes, 0, configs, 0, 1, count, 0)) {
            "eglChooseConfig failed: 0x${EGL14.eglGetError().toString(16)}"
        }
        check(count[0] > 0) { "no recordable EGL config" }
        return checkNotNull(configs[0])
    }

    private fun makeEncoderSurfaceCurrent() {
        check(EGL14.eglMakeCurrent(eglDisplay, eglSurface, eglSurface, eglContext)) {
            "eglMakeCurrent failed: 0x${EGL14.eglGetError().toString(16)}"
        }
    }

    private fun releaseOnRenderThread() {
        inputSurfaceTexture?.setOnFrameAvailableListener(null)
        inputSurface?.release()
        inputSurface = null
        inputSurfaceTexture?.release()
        inputSurfaceTexture = null

        if (eglDisplay != EGL14.EGL_NO_DISPLAY) {
            if (eglContext != EGL14.EGL_NO_CONTEXT && eglSurface != EGL14.EGL_NO_SURFACE) {
                EGL14.eglMakeCurrent(eglDisplay, eglSurface, eglSurface, eglContext)
                if (programId != 0) GLES20.glDeleteProgram(programId)
                if (textureId != 0) GLES20.glDeleteTextures(1, intArrayOf(textureId), 0)
            }
            EGL14.eglMakeCurrent(
                eglDisplay,
                EGL14.EGL_NO_SURFACE,
                EGL14.EGL_NO_SURFACE,
                EGL14.EGL_NO_CONTEXT,
            )
            if (eglSurface != EGL14.EGL_NO_SURFACE) {
                EGL14.eglDestroySurface(eglDisplay, eglSurface)
            }
            if (eglContext != EGL14.EGL_NO_CONTEXT) {
                EGL14.eglDestroyContext(eglDisplay, eglContext)
            }
            EGL14.eglReleaseThread()
            EGL14.eglTerminate(eglDisplay)
        }
        eglSurface = EGL14.EGL_NO_SURFACE
        eglContext = EGL14.EGL_NO_CONTEXT
        eglDisplay = EGL14.EGL_NO_DISPLAY
        programId = 0
        textureId = 0
    }

    private fun createProgram(vertexSource: String, fragmentSource: String): Int {
        val vertexShader = compileShader(GLES20.GL_VERTEX_SHADER, vertexSource)
        val fragmentShader = compileShader(GLES20.GL_FRAGMENT_SHADER, fragmentSource)
        val program = GLES20.glCreateProgram()
        check(program != 0) { "glCreateProgram failed" }
        GLES20.glAttachShader(program, vertexShader)
        GLES20.glAttachShader(program, fragmentShader)
        GLES20.glLinkProgram(program)
        val status = IntArray(1)
        GLES20.glGetProgramiv(program, GLES20.GL_LINK_STATUS, status, 0)
        val log = GLES20.glGetProgramInfoLog(program)
        GLES20.glDeleteShader(vertexShader)
        GLES20.glDeleteShader(fragmentShader)
        check(status[0] == GLES20.GL_TRUE) { "program link failed: $log" }
        return program
    }

    private fun compileShader(type: Int, source: String): Int {
        val shader = GLES20.glCreateShader(type)
        check(shader != 0) { "glCreateShader failed" }
        GLES20.glShaderSource(shader, source)
        GLES20.glCompileShader(shader)
        val status = IntArray(1)
        GLES20.glGetShaderiv(shader, GLES20.GL_COMPILE_STATUS, status, 0)
        val log = GLES20.glGetShaderInfoLog(shader)
        if (status[0] != GLES20.GL_TRUE) {
            GLES20.glDeleteShader(shader)
            error("shader compile failed: $log")
        }
        return shader
    }

    private fun throwOnGlError(operation: String) {
        val error = GLES20.glGetError()
        check(error == GLES20.GL_NO_ERROR) { "$operation GL error: 0x${error.toString(16)}" }
    }

    companion object {
        fun create(
            encoderSurface: Surface,
            cameraBufferSize: Size,
            outputSize: Size,
            initialRotationDegrees: Int,
            onError: (String) -> Unit,
        ): Result<CameraEncodingCompositor> {
            val compositor = CameraEncodingCompositor(
                encoderSurface = encoderSurface,
                cameraBufferSize = cameraBufferSize,
                outputSize = outputSize,
                initialRotationDegrees = initialRotationDegrees,
                onError = onError,
            )
            return compositor.initialize().map { compositor }
        }

        private fun floatBufferOf(vararg values: Float): FloatBuffer =
            ByteBuffer.allocateDirect(values.size * Float.SIZE_BYTES)
                .order(ByteOrder.nativeOrder())
                .asFloatBuffer()
                .apply {
                    put(values)
                    position(0)
                }

        private const val EGL_RECORDABLE_ANDROID = 0x3142
        private const val MATRIX_SIZE = 16
        private const val COORDINATES_PER_VERTEX = 2
        private const val VERTEX_COUNT = 4
        private const val INIT_TIMEOUT_SECONDS = 3L
        private const val RELEASE_TIMEOUT_SECONDS = 2L

        private const val VERTEX_SHADER = """
            attribute vec4 aPosition;
            attribute vec4 aTextureCoordinate;
            uniform mat4 uTextureMatrix;
            varying vec2 vTextureCoordinate;
            void main() {
                gl_Position = aPosition;
                vTextureCoordinate = (uTextureMatrix * aTextureCoordinate).xy;
            }
        """

        private const val FRAGMENT_SHADER = """
            #extension GL_OES_EGL_image_external : require
            precision mediump float;
            uniform samplerExternalOES uTexture;
            varying vec2 vTextureCoordinate;
            void main() {
                gl_FragColor = texture2D(uTexture, vTextureCoordinate);
            }
        """
    }
}

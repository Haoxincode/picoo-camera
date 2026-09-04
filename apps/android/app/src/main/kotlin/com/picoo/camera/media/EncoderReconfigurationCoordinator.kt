package com.picoo.camera.media

import android.os.SystemClock
import com.picoo.camera.jni.PicooNative

/**
 * Owns the native-encoder apply/ACK boundary from REQ-PICOO-MEDIA-007.
 *
 * This is runtime orchestration state, not UI state: Compose only observes the
 * returned completion result and never owns a pending Rust directive.
 */
class EncoderReconfigurationCoordinator {
    private data class PendingApply(
        val streamEpoch: Int,
        val targetHeight: Int,
        val directiveId: Long?,
        val deadlineMs: Long,
        val recoveryMessage: String? = null,
    )

    private data class CommittedApply(
        val width: Int,
        val height: Int,
        val streamEpoch: Int,
        val bitrateBps: Int,
    )

    sealed interface PollResult {
        data class Applied(
            val bitrateBps: Int,
            val actualHeight: Int,
        ) : PollResult
        data class Failed(val message: String) : PollResult
        data class Recovered(
            val bitrateBps: Int,
            val actualHeight: Int,
            val message: String,
        ) : PollResult
    }

    private var pending: PendingApply? = null
    private var committed: CommittedApply? = null

    val isPending: Boolean
        get() = pending != null

    fun beginLocal(
        senderHandle: Long,
        encoder: Camera2MediaEncoder,
        targetHeight: Int,
    ): Int {
        rememberCommitted(senderHandle, encoder)
        cancelPending(senderHandle)
        // A user/camera/capability transition takes precedence over ABR, but
        // supersession is explicit so Rust can roll back the pending ladder.
        PicooNative.readEncoderDirective(senderHandle)?.let { directive ->
            PicooNative.nackEncoderDirective(senderHandle, directive.id)
        }
        val epoch = PicooNative.beginStreamReconfiguration(senderHandle, targetHeight)
        if (epoch > 0) {
            encoder.prepareStreamEpoch(epoch)
            pending = PendingApply(
                epoch,
                targetHeight,
                directiveId = null,
                deadlineMs = SystemClock.elapsedRealtime() + APPLY_TIMEOUT_MS,
            )
        }
        return epoch
    }

    fun beginDirective(
        senderHandle: Long,
        encoder: Camera2MediaEncoder,
        directive: PicooNative.EncoderDirective,
    ): Boolean {
        if (pending != null) return false
        rememberCommitted(senderHandle, encoder)
        encoder.prepareStreamEpoch(directive.streamEpoch)
        pending = PendingApply(
            streamEpoch = directive.streamEpoch,
            targetHeight = directive.targetHeight,
            directiveId = directive.id,
            deadlineMs = SystemClock.elapsedRealtime() + APPLY_TIMEOUT_MS,
        )
        return true
    }

    fun poll(senderHandle: Long, encoder: Camera2MediaEncoder): PollResult? {
        val apply = pending
        if (apply == null) {
            if (encoder.state == CaptureState.Previewing && encoder.appliedStreamEpoch > 0) {
                rememberLatestCommitted(senderHandle, encoder)
                return null
            }
            if (encoder.state == CaptureState.Error) {
                return startCommittedRecovery(
                    senderHandle,
                    encoder,
                    encoder.lastError ?: "编码器运行失败",
                )
            }
            return null
        }
        if (encoder.state == CaptureState.Error) {
            val message = encoder.lastError ?: "编码器重建失败"
            return failOrRecover(senderHandle, encoder, apply, message)
        }
        if (SystemClock.elapsedRealtime() >= apply.deadlineMs) {
            return failOrRecover(
                senderHandle,
                encoder,
                apply,
                "编码器未在 3 秒内输出目标关键帧",
            )
        }
        if (encoder.state != CaptureState.Previewing ||
            encoder.appliedStreamEpoch != apply.streamEpoch
        ) {
            return null
        }

        val actualHeight = encoder.appliedEncoderHeight
        if (actualHeight <= 0) return null
        if (apply.directiveId != null && actualHeight != apply.targetHeight) {
            return failOrRecover(
                senderHandle,
                encoder,
                apply,
                "编码器未能应用目标 ${apply.targetHeight}p（实际 ${actualHeight}p）",
            )
        }

        val synced = if (apply.directiveId != null) {
            PicooNative.ackEncoderDirective(
                senderHandle,
                apply.directiveId,
                actualHeight,
            ) == 1
        } else {
            PicooNative.reportEncoderHeight(
                senderHandle,
                actualHeight,
                apply.streamEpoch,
            ) == 0
        }
        pending = null
        if (!synced) {
            return failOrRecover(senderHandle, encoder, apply, "无法同步编码器状态")
        }
        val bitrate = PicooNative.readSenderSnapshot(senderHandle).currentBitrateBps
        val size = encoder.profile.resolution
        committed = CommittedApply(size.width, actualHeight, apply.streamEpoch, bitrate)
        return apply.recoveryMessage?.let { message ->
            PollResult.Recovered(bitrate, actualHeight, message)
        } ?: PollResult.Applied(bitrate, actualHeight)
    }

    private fun cancelPending(senderHandle: Long) {
        pending?.let { finishFailed(senderHandle, it) }
        pending = null
    }

    fun abandonDisconnectedSession(senderHandle: Long) {
        pending?.let { finishFailed(senderHandle, it) }
        pending = null
        committed = null
    }

    private fun finishFailed(senderHandle: Long, apply: PendingApply) {
        if (apply.directiveId != null) {
            PicooNative.nackEncoderDirective(senderHandle, apply.directiveId)
        } else {
            PicooNative.cancelStreamReconfiguration(senderHandle, apply.streamEpoch)
        }
    }

    private fun rememberCommitted(senderHandle: Long, encoder: Camera2MediaEncoder) {
        if (committed != null) return
        val snapshot = PicooNative.readSenderSnapshot(senderHandle)
        val size = encoder.profile.resolution
        committed = CommittedApply(
            width = size.width,
            height = size.height,
            streamEpoch = snapshot.streamEpoch,
            bitrateBps = snapshot.currentBitrateBps,
        )
    }

    private fun failOrRecover(
        senderHandle: Long,
        encoder: Camera2MediaEncoder,
        apply: PendingApply,
        message: String,
    ): PollResult? {
        if (apply.recoveryMessage != null) {
            pending = null
            PicooNative.disconnect(senderHandle)
            encoder.stopPreview()
            return PollResult.Failed("$message；恢复上一视频配置失败，已断开连接")
        }
        finishFailed(senderHandle, apply)
        return startCommittedRecovery(senderHandle, encoder, message)
    }

    private fun startCommittedRecovery(
        senderHandle: Long,
        encoder: Camera2MediaEncoder,
        message: String,
    ): PollResult? {
        val rollback = committed
        if (rollback == null || rollback.streamEpoch <= 0) {
            pending = null
            PicooNative.disconnect(senderHandle)
            encoder.stopPreview()
            return PollResult.Failed("$message；没有可恢复的视频配置，已断开连接")
        }
        encoder.restoreCommittedConfiguration(
            width = rollback.width,
            height = rollback.height,
            streamEpoch = rollback.streamEpoch,
            bitrateBps = rollback.bitrateBps,
        )
        pending = PendingApply(
            streamEpoch = rollback.streamEpoch,
            targetHeight = rollback.height,
            directiveId = null,
            deadlineMs = SystemClock.elapsedRealtime() + APPLY_TIMEOUT_MS,
            recoveryMessage = message,
        )
        return null
    }

    private fun rememberLatestCommitted(senderHandle: Long, encoder: Camera2MediaEncoder) {
        val snapshot = PicooNative.readSenderSnapshot(senderHandle)
        if (encoder.appliedStreamEpoch != snapshot.streamEpoch) return
        val height = encoder.appliedEncoderHeight
        if (height <= 0) return
        val size = encoder.profile.resolution
        committed = CommittedApply(
            width = size.width,
            height = height,
            streamEpoch = snapshot.streamEpoch,
            bitrateBps = snapshot.currentBitrateBps,
        )
    }

    private companion object {
        const val APPLY_TIMEOUT_MS = 3_000L
    }
}

package com.picoo.camera.media

import com.picoo.camera.jni.PicooNative

/**
 * Executes Rust-owned encoder directives and reports native facts.
 *
 * Rust is the only authority that commits, rolls back, times out, or requests
 * recovery (REQ-PICOO-MEDIA-007/016). This coordinator only retains the native
 * configuration required to execute a recovery effect and derives UI results
 * from the committed Rust snapshot.
 */
class EncoderReconfigurationCoordinator {
    private data class PendingApply(
        val transactionId: Long,
        val streamEpoch: Int,
        val targetHeight: Int,
        val recoveryMessage: String? = null,
    )

    private data class CommittedApply(
        val width: Int,
        val height: Int,
        val streamEpoch: Int,
        val bitrateBps: Int,
    )

    sealed interface PollResult {
        data class Applied(val bitrateBps: Int, val actualHeight: Int) : PollResult
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
        val epoch = PicooNative.beginStreamReconfiguration(senderHandle, targetHeight)
        if (epoch <= 0) return 0
        val transactionId = PicooNative.encoderTransactionId(senderHandle, epoch)
        if (transactionId <= 0) return 0
        encoder.prepareStreamEpoch(epoch)
        pending = PendingApply(transactionId, epoch, targetHeight)
        return epoch
    }

    fun beginDirective(
        senderHandle: Long,
        encoder: Camera2MediaEncoder,
        directive: PicooNative.EncoderDirective,
    ): Boolean {
        if (pending != null) return false
        rememberCommitted(senderHandle, encoder)
        if (directive.kind == RECOVERY_KIND) {
            return beginRecovery(senderHandle, encoder, directive, "编码器运行失败") == null
        }
        encoder.prepareStreamEpoch(directive.streamEpoch)
        pending = PendingApply(
            transactionId = directive.id,
            streamEpoch = directive.streamEpoch,
            targetHeight = directive.targetHeight,
        )
        return true
    }

    /** Report that a directive cannot be executed before a native generation starts. */
    fun rejectBeforeStart(senderHandle: Long, directive: PicooNative.EncoderDirective) {
        PicooNative.reportEncoderFailed(senderHandle, directive.id, 0)
    }

    fun poll(senderHandle: Long, encoder: Camera2MediaEncoder): PollResult? {
        if (PicooNative.readSenderSnapshot(senderHandle).status == PicooNative.STATUS_DISCONNECTED) {
            abandonDisconnectedSession()
            return null
        }
        val rustDirective = PicooNative.readEncoderDirective(senderHandle)
        val apply = pending
        if (rustDirective?.kind == RECOVERY_KIND && rustDirective.id != apply?.transactionId) {
            return beginRecovery(
                senderHandle,
                encoder,
                rustDirective,
                encoder.lastError ?: "编码器调整超时",
            )
        }

        if (apply == null) {
            if (encoder.state == CaptureState.Error) {
                val outcome = PicooNative.reportEncoderFailed(
                    senderHandle,
                    0,
                    encoder.encoderGeneration,
                )
                return handleFailureOutcome(
                    senderHandle,
                    encoder,
                    outcome,
                    encoder.lastError ?: "编码器运行失败",
                )
            }
            if (encoder.state == CaptureState.Previewing && encoder.appliedStreamEpoch > 0) {
                rememberLatestCommitted(senderHandle, encoder)
            }
            return null
        }

        if (encoder.state == CaptureState.Error) {
            val outcome = PicooNative.reportEncoderFailed(
                senderHandle,
                apply.transactionId,
                encoder.encoderGeneration,
            )
            return handleFailureOutcome(
                senderHandle,
                encoder,
                outcome,
                encoder.lastError ?: "编码器重建失败",
            )
        }

        val activeTransaction = PicooNative.encoderTransactionId(senderHandle, apply.streamEpoch)
        if (activeTransaction == apply.transactionId) return null

        val snapshot = PicooNative.readSenderSnapshot(senderHandle)
        val committedByRust = activeTransaction == 0L &&
            snapshot.streamEpoch == apply.streamEpoch &&
            snapshot.activeHeight == apply.targetHeight &&
            encoder.state == CaptureState.Previewing &&
            encoder.appliedStreamEpoch == apply.streamEpoch &&
            encoder.appliedEncoderHeight == apply.targetHeight
        if (!committedByRust) {
            pending = null
            return if (snapshot.status == PicooNative.STATUS_DISCONNECTED) {
                PollResult.Failed("编码器恢复失败，连接已断开")
            } else {
                PollResult.Failed("编码器调整未能提交")
            }
        }

        pending = null
        val size = encoder.profile.resolution
        committed = CommittedApply(
            width = size.width,
            height = apply.targetHeight,
            streamEpoch = apply.streamEpoch,
            bitrateBps = snapshot.currentBitrateBps,
        )
        return apply.recoveryMessage?.let { message ->
            PollResult.Recovered(snapshot.currentBitrateBps, apply.targetHeight, message)
        } ?: PollResult.Applied(snapshot.currentBitrateBps, apply.targetHeight)
    }

    fun abandonDisconnectedSession() {
        pending = null
        committed = null
    }

    private fun handleFailureOutcome(
        senderHandle: Long,
        encoder: Camera2MediaEncoder,
        outcome: Int,
        message: String,
    ): PollResult? = when (outcome) {
        FAILURE_ROLLED_BACK -> {
            pending = null
            PollResult.Failed(message)
        }
        FAILURE_RECOVERY_REQUESTED -> {
            val recovery = PicooNative.readEncoderDirective(senderHandle)
            if (recovery?.kind == RECOVERY_KIND) {
                beginRecovery(senderHandle, encoder, recovery, message)
            } else {
                pending = null
                PollResult.Failed("$message；无法取得恢复配置")
            }
        }
        FAILURE_DISCONNECTED -> {
            pending = null
            encoder.stopPreview()
            PollResult.Failed("$message；恢复上一视频配置失败，已断开连接")
        }
        else -> null
    }

    private fun beginRecovery(
        senderHandle: Long,
        encoder: Camera2MediaEncoder,
        directive: PicooNative.EncoderDirective,
        message: String,
    ): PollResult? {
        val rollback = committed
        if (rollback == null ||
            rollback.streamEpoch != directive.streamEpoch ||
            rollback.height != directive.targetHeight
        ) {
            pending = null
            PicooNative.reportEncoderFailed(senderHandle, directive.id, 0)
            encoder.stopPreview()
            return PollResult.Failed("$message；没有可恢复的视频配置，已断开连接")
        }
        encoder.restoreCommittedConfiguration(
            width = rollback.width,
            height = rollback.height,
            streamEpoch = directive.streamEpoch,
            bitrateBps = directive.targetBitrateBps,
        )
        pending = PendingApply(
            transactionId = directive.id,
            streamEpoch = directive.streamEpoch,
            targetHeight = directive.targetHeight,
            recoveryMessage = message,
        )
        return null
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

    private fun rememberLatestCommitted(senderHandle: Long, encoder: Camera2MediaEncoder) {
        val snapshot = PicooNative.readSenderSnapshot(senderHandle)
        if (encoder.appliedStreamEpoch != snapshot.streamEpoch) return
        val height = encoder.appliedEncoderHeight
        if (height <= 0 || height != snapshot.activeHeight) return
        val size = encoder.profile.resolution
        committed = CommittedApply(
            width = size.width,
            height = height,
            streamEpoch = snapshot.streamEpoch,
            bitrateBps = snapshot.currentBitrateBps,
        )
    }

    private companion object {
        const val RECOVERY_KIND = 4
        const val FAILURE_ROLLED_BACK = 1
        const val FAILURE_RECOVERY_REQUESTED = 2
        const val FAILURE_DISCONNECTED = 3
    }
}

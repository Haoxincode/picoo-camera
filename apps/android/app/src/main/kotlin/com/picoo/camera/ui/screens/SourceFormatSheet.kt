package com.picoo.camera.ui.screens

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import com.picoo.camera.media.VideoSourceFormat
import com.picoo.camera.ui.components.PicooSheet
import com.picoo.camera.ui.components.PicooSheetRow

/** Plain request selection; capability queries and transactions belong to the ViewModel. */
@Composable
internal fun SourceFormatSheet(
    selected: VideoSourceFormat?,
    candidates: List<VideoSourceFormat>?,
    onDismiss: () -> Unit,
    onSelect: (VideoSourceFormat) -> Unit,
    preparationError: String? = null,
) {
    PicooSheet(
        title = "视频源格式",
        description = "按当前镜头、手机方向和接收端能力选择。不会自动降低规格。",
        onDismiss = onDismiss,
    ) {
        when {
            preparationError != null -> Text(preparationError)
            candidates == null -> Text("正在检查可用格式…")
            candidates.isEmpty() -> Text("当前没有可用格式，请检查镜头、手机方向和接收端。")
            else -> candidates.forEach { source ->
                PicooSheetRow(
                    title = source.label,
                    selected = source == selected,
                    onClick = { onSelect(source) },
                )
            }
        }
        PicooSheetRow(title = "取消", onClick = onDismiss)
    }
}

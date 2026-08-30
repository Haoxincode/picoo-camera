package com.picoo.camera.ui.components

import androidx.annotation.DrawableRes
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import com.picoo.camera.R

/**
 * ARCH-PICOO-UI-002 semantic icon layer.
 *
 * Feature code names product intent; this adapter alone maps intent to the pinned Reicon source
 * glyph in `assets/icons/reicon/manifest.json`.
 */
enum class Reicon(@DrawableRes internal val drawableRes: Int) {
    Settings(R.drawable.reicon_settings),
    ReceiverDevice(R.drawable.reicon_desktop),
    Discovering(R.drawable.reicon_radio),
    Network(R.drawable.reicon_wifi),
    SecureConnection(R.drawable.reicon_shield_check),
    SwitchCamera(R.drawable.reicon_camera_rotate),
    Exposure(R.drawable.reicon_sun),
    Mirror(R.drawable.reicon_flip_horizontal),
    InteractionLock(R.drawable.reicon_lock),
    InteractionUnlock(R.drawable.reicon_unlock),
    RefreshDiscovery(R.drawable.reicon_refresh),
    ResetExposure(R.drawable.reicon_refresh),
    RepairFrame(R.drawable.reicon_refresh),
    StopStream(R.drawable.reicon_stop),
    Disconnect(R.drawable.reicon_phone_off),
    Overheat(R.drawable.reicon_flame),
    MoreActions(R.drawable.reicon_more_horizontal),
    NavigateBack(R.drawable.reicon_chevron_left),
    Rejected(R.drawable.reicon_xmark),
    Expired(R.drawable.reicon_clock),
}

@Composable
fun ReiconIcon(
    icon: Reicon,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    tint: Color = LocalContentColor.current,
) {
    Icon(
        painter = painterResource(icon.drawableRes),
        contentDescription = contentDescription,
        modifier = modifier,
        tint = tint,
    )
}

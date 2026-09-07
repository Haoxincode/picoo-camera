package com.picoo.camera.media

/** Foreground camera access for instrumentation; absent from release builds. */
class MediaProbeActivity : android.app.Activity() {
    override fun onCreate(savedInstanceState: android.os.Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setContentView(android.widget.TextView(this).apply {
            text = "正在进行相机测试\n\n请保持手机解锁\n测试完成后自动关闭\n不会保存摄像头画面"
            textSize = 20f
            gravity = android.view.Gravity.CENTER
        })
    }
}

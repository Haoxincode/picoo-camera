# 小米 15 原生格式样本

2026-09-07 在小米 15（24129PN74C，Android 16）运行 NativeCodecContractTest 生成。仅含 Canvas 合成图像，经生产 CameraEncodingCompositor 与 NativeVideoEncoder，实际组件为 c2.qti.avc.encoder / c2.qti.hevc.encoder。配置为 JNI 转换的标准 avcC/hvcC；AU 经共享 bitstream 明确从 Annex B 转四字节长度格式。无相机拍摄或用户影像。

文件名 codec（1 AVC / 2 HEVC）-可见高度-请求 fps；HEVC 720p 实际 coded height 为 736，两 codec 1080p 为 1088。复现见 verification/native-media/README.md。不证明相机或持续帧率。关联 REQ-PICOO-MEDIA-049。

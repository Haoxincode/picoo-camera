# 原生媒体验证

## iOS 生产编码器的 Apple 原生路径

在 Apple Silicon macOS、项目使用的 Xcode 工具链上运行：

```sh
mkdir -p target/verification
xcrun swiftc -swift-version 6 -default-isolation MainActor \
  -strict-concurrency=complete -warnings-as-errors -target arm64-apple-macosx15.0 \
  apps/ios/PicooCamera/VideoEncoder.swift \
  apps/ios/PicooCamera/VideoSourceFormat.swift \
  apps/ios/PicooCamera/VideoEncoderOutput.swift \
  apps/ios/PicooCamera/VideoEncoderPipeline.swift \
  verification/native-media/ios-encoder-harness.swift \
  -o target/verification/ios-encoder-harness
target/verification/ios-encoder-harness target/verification/ios-encoder-output
cargo run -p picoo-bitstream --example check_native_encoder -- target/verification/ios-encoder-output
```

harness 直接编译生产 Swift，检查过小/过大/转置输入及未知色彩拒绝、八种 codec/尺寸/fps 请求的原生硬件输出和输入快照；Rust 检查器复用正式位流解析器验证 avcC/hvcC、闭合 IDR、可见尺寸和 BT.709 limited。生成文件是合成画面的诊断产物，可删除。CPU mapping 仅用于初始化测试像素。

关联 REQ-PICOO-MEDIA-045、064、065；每组还以新的epoch/原生世代验证方向0→90→0重建，检查AU始终保留所属回调配置。这不是 iPhone 摄像头、持续帧率、画质、热稳态或四组合验收；完整边界和已记录结果见 [Apple 原生媒体研究](../../docs/research/next-apple-gpu.md)。

## 小米合成编码样本与 Mac 原生解码

在连接的 Android 真机安装当前 debug App 与 instrumentation 包，运行 `com.picoo.camera.media.NativeCodecContractTest`。八种配置每组保存三张合成输出中的第一张 sync AU 与标准记录到 App 私有 `files/native-codec-probe`；只包含 Canvas 色块，无相机影像。用 `adb exec-out run-as com.picoo.camera.debug cat files/native-codec-probe/<文件名>` 取得 `.config` 和 `.native-au` 后运行：

```sh
cargo run -p picoo-bitstream --example check_native_encoder -- /tmp/picoo-xiaomi-native-output --annex-b
cargo run -p picoo-media-decode --example check_native_formats -- /tmp/picoo-xiaomi-native-output
```

第一条显式解释 Android Annex B，检查实际位流并写入规范 `.au`；没有格式猜测。第二条使用正式平台 Decoder，验证八组实际参数、原生图像及原始 token。Windows 需加 `--features windows-mf` 并在 Windows 原生环境运行。Apple harness 的输出目录可直接传给第二条。

REQ-PICOO-MEDIA-049：这些合成输入证明原生 API 与完整参数事实，不证明摄像头、网络、持续帧率、画质或热稳态；程序不自动发布任何 Decoder offers。样本由 picoo-media-decode/probes 的 apple-native-formats/xiaomi-native-formats 管理，不能替代运行设备的探测。

## 实际 Camera2 帧率

调试 App 的 `MediaProbeActivity` 仅存在于 debug，受系统 DUMP 权限保护；instrumentation 通过 shell 启动。重装后确认相机权限，然后运行 `com.picoo.camera.media.NativeCameraCaptureContractTest`。入口显示测试提示，结束自动关闭；只采集有界 PTS 与配置，不持有或保存相机画面。`PicooCameraProbe` 日志区分请求 fps、实际 PTS 帧率和约3秒测量窗口。

该测试覆盖后置横向 Camera2→生产 compositor→生产 MediaCodec 八组合；不能作为前摄、竖持、网络、长时热稳态或录像并发证明。关联 REQ-PICOO-MEDIA-050。

## 当前原生 Decoder 能力探测

`cargo run -p picoo-media-decode --example probe_native_decoder` 在调用线程创建正式 native factory，逐个探测自有合成码流；Windows 加 `--features windows-mf`。结果包含每条实际成功的存储/crop/tier/level/fps，不从样本静态生成能力，不启用软件 fallback。没有成功项或 reset 失败返回错误。REQ-PICOO-MEDIA-052；不作为持续帧率或热稳态结果。

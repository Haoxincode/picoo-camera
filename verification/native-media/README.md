# 原生媒体验证

## iOS 生产编码器的 Apple 原生路径

在 Apple Silicon macOS、项目使用的 Xcode 工具链上运行：

```sh
mkdir -p target/verification
xcrun swiftc -swift-version 6 -default-isolation MainActor \
  -strict-concurrency=complete -warnings-as-errors -target arm64-apple-macosx15.0 \
  apps/ios/PicooCamera/VideoEncoder.swift \
  apps/ios/PicooCamera/VideoEncoderOutput.swift \
  apps/ios/PicooCamera/VideoEncoderPipeline.swift \
  verification/native-media/ios-encoder-harness.swift \
  -o target/verification/ios-encoder-harness
target/verification/ios-encoder-harness target/verification/ios-encoder-output
cargo run -p picoo-bitstream --example check_native_encoder -- target/verification/ios-encoder-output
```

harness 直接编译生产 Swift，检查未知色彩输入拒绝、八种 codec/尺寸/fps 请求的原生硬件输出和输入快照；Rust 检查器复用正式位流解析器验证 avcC/hvcC、闭合 IDR、可见尺寸和 BT.709 limited。生成文件是合成画面的诊断产物，可删除。CPU mapping 仅用于初始化测试像素。

关联 REQ-PICOO-MEDIA-045。这不是 iPhone 摄像头、持续帧率、画质、热稳态或四组合验收；完整边界和已记录结果见 [Apple 原生媒体研究](../../docs/research/next-apple-gpu.md)。

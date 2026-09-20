# 原生硬件位流样本

2026-09-06 在 Apple M4 / macOS 26.6.2 / Xcode 26.6 由仓库 `scripts/probes/apple_native_media.swift` 生成。

运行 probe 时传入输出目录，可重建 AVC High / HEVC Main 的 1280×720、请求 30fps 配置记录和第一张随机访问 AU。输入为固定合成 NV12（Y=96、UV=128），不含用户图像。文件命名中的 idr 由测试验证，不依赖编码器 flag。

`*-config.bin` 是 avcC/hvcC atom 内容，不含 box header；`*-idr.bin` 是 CoreMedia 压缩 sample，4-byte big-endian NAL length。probe 同时用真实 VideoToolbox 硬件 Decoder 解码成功并完成 Metal 双平面映射。样本与探针代码适用仓库 MIT OR Apache-2.0。

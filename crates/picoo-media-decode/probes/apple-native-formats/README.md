# Apple 原生格式样本

2026-09-07 在 M4 上使用 verification/native-media/ios-encoder-harness.swift，直接编译生产 iOS VideoEncoder 实现生成。合成 BT.709 limited 420v 输入，无用户影像；标准配置 atom 与四字节长度 IDR 原样保留。文件名为 codec（1 AVC / 2 HEVC）-可见高度-请求 fps。两 codec 的 1080 可见高度对应 1088 编码高度。

用于参数事实与原生 Decoder 回归，不是运行设备的能力证明，不证明持续 60fps。复现命令见 verification/native-media/README.md。关联 REQ-PICOO-MEDIA-045、049。

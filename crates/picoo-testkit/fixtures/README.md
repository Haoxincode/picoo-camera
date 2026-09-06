# 原生解码测试素材

`avc-1080p-red-idr.h264`：2026-09-06 在 Apple M4 / macOS 26.6.2 上通过系统 VideoToolbox 硬件 Encoder 生成的单张纯红色 1920×1080 AVC High Annex B IDR，包含参数集。用于 REQ-PICOO-MEDIA-027/028 的显式源配置切换解码回归，不作为持续编码性能或真实相机验收。

```sh
ffmpeg -hide_banner -loglevel error -f lavfi -i color=c=red:s=1920x1080:r=30 -frames:v 1 -c:v h264_videotoolbox -allow_sw 0 -profile:v high -b:v 6000000 -pix_fmt nv12 -f h264 avc-1080p-red-idr.h264
```

素材由合成颜色生成，不含第三方拍摄内容。FFmpeg 仅用于一次性生成测试输入，不增加产品依赖。

## 显式 BT.709 的原生 AVC 源

`avc-64x64-bt709-idr.h264`、`avc-1280x720-bt709-idr.h264`、`avc-1920x1080-bt709-idr.h264` 由 M4 / macOS 26.6.2 / Xcode 26.6 生成。命令：

```sh
swiftc -warnings-as-errors scripts/probes/apple_avc_fixture.swift -o /tmp/picoo-avc-fixture
/tmp/picoo-avc-fixture crates/picoo-testkit/fixtures
```

生成器只使用系统 VideoToolbox 硬件 AVC High，显式关闭重排并提交 BT.709 limited、PAR 1:1。合成红色 NV12 取 Y/U/V=63/102/240。原生 SPS 实测包含 BT.709 色彩，但省略了 square SAR；位流解析必须保留未知 PAR，不能把 Encoder 的请求值冒充 SPS 中存在的事实。Decoder 检查 CoreVideo 的实际 clean aperture 和 nominal display size（square pixels）后建立输出 PAR。1080p 的编码高度为 1088，可见高度为 1080。

这些样本验证 Decoder→FrameBus→GPU→CPU 的色彩和资源合同，不代表相机实时吞吐。旧未声明 BT.709 的素材保留用于未知色彩拒绝和其他明确的位流回归，不作为原生 GPU 路径的已知色彩输入。

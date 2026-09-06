# 原生解码测试素材

`avc-1080p-red-idr.h264`：2026-09-06 在 Apple M4 / macOS 26.6.2 上通过系统 VideoToolbox 硬件 Encoder 生成的单张纯红色 1920×1080 AVC High Annex B IDR，包含参数集。用于 REQ-PICOO-MEDIA-027/028 的显式源配置切换解码回归，不作为持续编码性能或真实相机验收。

```sh
ffmpeg -hide_banner -loglevel error -f lavfi -i color=c=red:s=1920x1080:r=30 -frames:v 1 -c:v h264_videotoolbox -allow_sw 0 -profile:v high -b:v 6000000 -pix_fmt nv12 -f h264 avc-1080p-red-idr.h264
```

素材由合成颜色生成，不含第三方拍摄内容。FFmpeg 仅用于一次性生成测试输入，不增加产品依赖。

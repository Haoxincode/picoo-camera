# 合成MP4 sample description参考

这些stsd box从Apple原生AVAssetWriter生成的单视频轨文件提取，源为仓库`crates/picoo-media-decode/probes/apple-native-formats`中的合成AU/config。生成器为`verification/native-media/apple-mux-probe.swift`，本机2026-09-08重新验证八组合逐AU字节与PTS一致。

用于Windows MF mux研究的显式sample description输入，不是生产动态配置生成器。包含完整stsd box（box header、version/flags、entry count、avc1/hvc1与avcC/hvcC），不含媒体像素或真实摄像头影像。每个文件对应相同名称的源codec/尺寸/fps配置。

提取路径为moov/trak/mdia/minf/stbl/stsd；解析box size时同时处理32位长度、size=1的64位长度与size=0到容器末尾。Windows是否接纳、压缩VCL和PTS是否保留，必须由Windows原生probe确认，不能用Apple生成成功替代。

复现：先运行Apple mux probe，再执行`python3 verification/native-media/extract-mp4-sample-descriptions.py <生成的MP4目录> <新的stsd目录>`。提取器无覆盖写入，只有合成输入可用于此研究目录。

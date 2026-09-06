# Next 位流依赖核对（2026-09-06）

关联 ARCH-PICOO-MEDIA-002、REQ-PICOO-NEXT-003/023、REQ-PICOO-BITSTREAM-002。

| 候选 | 当前发布/维护 | 许可与工具链 | 适用性 |
| --- | --- | --- | --- |
| h264-reader | 0.8.0，2025-01-28 发布；仓库未归档，最近 push 2026-07-08 | MIT/Apache-2.0；未声明 MSRV | AVC 专用解析，不能覆盖 HEVC 配置；暂不同时引入两套 AVC 解释 |
| scuffle-h264 / scuffle-h265 | 均 0.2.2，2025-05-17 发布；scuffle 仓库未归档，最近 push 2026-04-19 | 包内 MIT OR Apache-2.0，edition 2024 要求至少 Rust 1.85；未声明更完整 MSRV，使用项目 stable 验证 | 采用其 AVCDecoderConfigurationRecord::parse/build、HEVCDecoderConfigurationRecord::demux/mux；纯 Rust，无平台 SDK、C codec 或 GPU 依赖 |
| h265-reader | crates.io 无该包（404） | 不适用 | 不凭相似名称虚构依赖 |
| oxideav-h265 | 0.0.10，2026-09-01 发布 | MIT，Rust 1.80 | 约 1.65 MB 源包且包含 decoder scaffold，范围大于配置解释；不引入软件解码体系 |

scuffle 源包大小分别 29,937 / 56,631 bytes，是压缩发布源码大小，不是二进制增长。正常依赖为 bytes、byteorder、bitflags（HEVC）、nutype-enum、scuffle-bytes-util、scuffle-expgolomb、scuffle-workspace-hack；docs/changelog feature 不启用。实测构建与依赖树附于实施记录。

边界：第三方负责标准配置记录解释/写入；Picoo 负责协商格式、大小/数量上限、消费完毕检查及受支持 profile/NAL 类型约束。通用库允许比产品更多的结构，不能直接把“parse 成功”当作配置准入。NAL header/RAP 映射是 Picoo 最小恢复语义边界，不能实现完整软件解码器；HEVC header 在 scuffle 中为私有模块，不能使用不存在的公开 API。

输入限定单层、8-bit 4:2:0；AU 内多个 picture、未知 VCL、BLA/保留 IRAP 必须明确拒绝，CRA 与 IDR、RASL 与 RADL 分开表达。完整 slice dependency/无 B 帧验证仍需平台 actual 配置和码流回归，头部分类本身不能证明全部约束。

来源：crates.io 对应包与 `/dependencies` API、下载的 0.2.2 Cargo.toml 和 src/config.rs；GitHub 仓库元数据。复核地址：

- https://crates.io/crates/scuffle-h264/0.2.2
- https://crates.io/crates/scuffle-h265/0.2.2
- https://github.com/scufflecloud/scuffle
- https://crates.io/crates/h264-reader/0.8.0
- https://github.com/dholroyd/h264-reader
- https://crates.io/crates/oxideav-h265/0.0.10

## 构建结果

锁文件新增 7 个 registry 包：scuffle-h264/h265 0.2.2、scuffle-bytes-util/expgolomb 0.1.5、nutype-enum 0.1.5、bytestring 1.5.1、scuffle-workspace-hack 0.1.0。workspace-hack 0.1.0 是无依赖空包，未把 Scuffle 工作区完整依赖带入。未开启 docs feature。Mac stable 构建通过，Android aarch64、iOS aarch64、Windows MSVC x86_64 的本 crate `cargo check` 通过；Windows 仅纯 Rust 库类型检查，不是桌面链路构建。

测试中的 AVC 高 profile 可选扩展缺失按标准库允许的合法表示处理。Picoo 不编写宽泛 SPS parser：配置对象只声明头部与存储验证，不证明全部图像语义；真实平台 Decoder 与完整 offer 准入仍要验证参数集描述的尺寸、颜色、无 B 帧等合同。

## SPS 几何与 VUI 事实

SPS 解析采用 `h264-reader = 0.8.0`，使用公开 `SeqParameterSet::from_bits(RefNal::rbsp_bits())`，包括 VUI、HRD 和 RBSP 结尾检查。其源码对 Exp-Golomb 的前导零限制为 31、POC 循环数量限制为 255、HRD CPB 数量限制为 32、scaling-list delta 限制为 [-128,127]。Picoo 不自行实现 SPS 或 Exp-Golomb parser。

未采用 scuffle-h264 0.2.2 的 SPS API：新增 fuzz 在 scuffle-expgolomb 0.1.5 的 `result - 1` 触发了整数下溢 panic；崩溃输入已保存为 `sps-exp-golomb-overflow.bin`。该路径未接入生产 Decoder。Scuffle 继续仅用于此前已采用的配置 record API，不再让不可信 SPS 进入其 SPS parser，也不以 catch_unwind 包裹风险。

h264-reader 0.8.0 于 2025-01-28 发布，仓库未归档，最近 push 为 2026-07-08；MIT/Apache-2.0。未声明完整 MSRV，依赖 bitstream-io 2.6.0 声明 Rust 1.79，当前 stable 构建验证另记。纯 Rust，不引入 codec、GPU 或平台 SDK；适合四端共享解析，目标构建仍按实际结果记录。发布源码压缩包 67,807 bytes，锁文件增加 h264-reader、bitstream-io 2.6.0、hex-slice、rfc6381-codec、four-cc、mp4ra-rust、mpeg4-audio-const 七个包；后四个是 codec 标识工具的依赖，不是音频解码器，最终二进制增量尚未量测。

Picoo 的 `AvcSpsFacts` 限制输入 64 KiB、progressive 8-bit 4:2:0 与 8192 编码尺寸上限；所有宏块尺寸和 crop 运算采用 checked arithmetic，不调用第三方尺寸便捷函数。PAR 与 color 未指定时保留 None/CICP unspecified，不默认改为方形像素或 BT.709。chroma location 缺省 0 来自 AVC E.2.1 的明确推导规则。

M4 原生 1080p AVC fixture 的编码尺寸为 1920×1088、可见尺寸为 1920×1080；该差异必须保留到原生帧元数据，不能从 CVPixelBuffer 的可见 allocation 尺寸反推编码尺寸。

参考：https://crates.io/crates/h264-reader/0.8.0 、https://github.com/dholroyd/h264-reader 。

# 小米 15 产品起始码率样本

2026-09-07在小米15（24129PN74C，Android16）由当前NativeCodecContractTest生成，仅含Canvas合成色块，经生产CameraEncodingCompositor与NativeVideoEncoder。AVC/HEVC都使用Core的bitrateInitialForHeight：720p为3 Mbps，1080p为6 Mbps；每个codec覆盖30/60fps。

配置由JNI转换为标准avcC/hvcC；原生Annex B AU通过既有 `cargo run -p picoo-bitstream --example check_native_encoder -- <目录> --annex-b` 校验闭合IDR、参数集身份、真实尺寸与BT.709 limited，并生成四字节长度AU。无相机或用户影像。

小米HEVC在该码率下选择Main tier；高码率对照样本位于[原生格式样本](../xiaomi-native-formats/README.md)。720p仍使用736行编码存储。不能从Main profile或高码率High tier样本推导低码率组合能力，Receiver必须实际probe后才广告对应offer。

关联REQ-PICOO-MEDIA-052。样本只验证实际格式与原生输出，不能代替持续fps、热稳态或任意码率范围验收。

# Picoo HEVC 解析依赖补丁

REQ-PICOO-BITSTREAM-002 / NEXT-025。来源与校验和见 ORIGIN.json。保留上游标准解析 API，补充进入算术或分配前的范围校验；Picoo 产品准入仍位于 picoo-bitstream。升级时逐项比较并删除上游已覆盖的补丁。

修改范围：sps/mod.rs、pcm.rs、scaling_list.rs、sps_scc_extension.rs、vui_parameters/mod.rs 与 rbsp_trailing_bits.rs（校验对齐零位）。SPS 上游测试内容移到 sps/tests.rs，仍属于原 tests 模块。bounds.rs 拥有内部 checked crop。回归随 picoo-bitstream workspace 测试运行。此补丁不宣称整个第三方公共 API 已完成审计。

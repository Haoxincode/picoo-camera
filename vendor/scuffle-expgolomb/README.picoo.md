# Picoo 本地依赖补丁

REQ-PICOO-BITSTREAM-002 / NEXT-025。保留 Scuffle 标准整数解释 API，修复超长前导零和有符号范围溢出，不新增产品协议或软件 codec。

来源及压缩发布包校验和见 ORIGIN.json。仅修改 src/lib.rs 的读取方法；上游升级时比较并移除已被上游覆盖的补丁。回归在 picoo-bitstream/tests/exp_golomb_bounds.rs，随 workspace 测试运行。写入 API 未在本补丁范围。

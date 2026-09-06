# GPUI 原生回归的属性测试依赖

关联 REQ-PICOO-NEXT-016 / REQ-PICOO-GPU-008。

Windows CI 34053537489 编译 GPUI test-support 时发现锁定的 proptest 1.6.0 没有 RngSeed::Fixed 和 Config::rng_seed。gpui-pre 0.3.3 的测试支持实际调用这两个 API，单独发布包在较新 proptest 下可编译，仓库中四个精确旧版本约束则强制选择 1.6.0。

复用现有 proptest，统一 workspace 测试依赖为 1.11.0，不修改 GPUI 的随机种子语义，也不关闭 shader 回归。核对已下载官方发布包：最低 Rust 1.85，MIT OR Apache-2.0，现有平台 Rust 基线满足；RngSeed 与 rng_seed API 均存在。它只服务属性测试及显式 GPUI test-support，不加入产品 normal feature 图。依赖变化复用已有 rand 0.9 / rand_chacha 0.9，rand_xorshift 更新为 0.4；不引入另一套测试框架。固定版本集中在 workspace，五个测试调用方不再各自锁定不同约束。

验证范围为 packet、frame-hub、jitter、protocol、testkit 的现有属性/行为回归，以及 Windows GPUI 原生 shader 测试。库或测试类型检查不能代替 Windows 实际绘制结果。

本机五个 crate 的现有回归：116 passed、0 failed、2 ignored。忽略项不计为通过；Windows GPUI 原生 shader 仍待修复提交后的 CI 执行。

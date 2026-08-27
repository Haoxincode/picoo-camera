# Agent 指令

这些规则适用于在本仓库工作的自动化编码 Agent。

## 核心原则

这个项目优先考虑概念对齐和架构正确性。

代码实现不是第一事实源。更重要的是先在第一性原理、领域语义、Design Specs、追溯 ID 和架构边界上达成一致，然后再判断生成代码是否可以接受。

如果 Agent 生成的代码质量不够好、不符合已经对齐的架构，或者把系统推向错误抽象，不应该因为它已经存在就继续保留。Agent 应该回退一步，更新相应 Design Spec，删除不合适的实现，并从架构层面重新设计和实现。

宁可保留一个小但符合概念模型的实现，也不要接受一个看似功能更多但架构方向错误的实现。

## Design Specs 上下文

设计规范、需求分解、追溯 ID、代码映射和核心术语以 [docs/design-specs/context.md](docs/design-specs/context.md) 为准。

Agent 在开始设计或实现前，应先读取这份 context，并按其中的规则组织后续工作。

只要实现工作引入了产品行为，就应该能追溯到稳定的 Requirement ID，并能继续关联到相关 Use Case 或 Architecture。

长期 Use Case 和 Architecture 应按 `context.md` 写成场景、意义、范围、边界和约束，不要写成阶段计划、实现流水账或外部项目复盘。

## 工作顺序

处理设计或实现工作时：

1. 先澄清需求，或从已有 Design Specs 中推导需求。
2. 按 `docs/design-specs/context.md` 区分 Use Case、Architecture 和 Requirement。
3. 在自行实现通用能力、协议适配、基础设施或用户界面组件前，先搜索仓库已有能力以及可以直接复用的外部包、官方 SDK 和成熟组件，并记录关键候选与适用性判断。
4. 引入产品行为的实现必须映射到稳定 Requirement ID，并通过对应 acceptance、测试或验证结果检查。
5. 验证实现是否仍然符合 Use Case 和 Architecture。如果不符合，修正设计或重建实现，不要在错误形态上继续叠加补丁。

## 语言

项目设计规范和内部推理文档默认使用中文。只有当来源材料是英文 API、标准或外部规范，并且保留原始术语更重要时，才使用英文。

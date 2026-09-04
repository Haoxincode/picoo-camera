# Agent 指令

这些规则适用于在本仓库工作的自动化编码 Agent。

## 核心原则

这个项目优先考虑概念对齐和架构正确性。

代码实现不是第一事实源。更重要的是先在第一性原理、领域语义、Design Specs、追溯 ID 和架构边界上达成一致，然后再判断生成代码是否可以接受。

如果 Agent 生成的代码质量不够好、不符合已经对齐的架构，或者把系统推向错误抽象，不应该因为它已经存在就继续保留。Agent 应该回退一步，更新相应 Design Spec，删除不合适的实现，并从架构层面重新设计和实现。

宁可保留一个小但符合概念模型的实现，也不要接受一个看似功能更多但架构方向错误的实现。

## 允许破坏性变更

项目完全自控，无需兼容历史版本。旧状态都可以作废，不要写迁移或降级路径。直接改当前实现并更新 Design Spec；重装、重新配对是预期行为。

## 文件规模

源文件超过 800 行必须按概念边界拆分，不得继续在同一文件里叠加职责。按模块、类型、协议面或平台适配切开，不要为凑行数做无语义的机械切分。历史产品基线文档除外。

## 成熟生态优先

通用能力、协议适配、基础设施、媒体处理和用户界面组件应优先采用官方 SDK、平台原生能力或维护活跃、许可兼容、跨目标可构建的成熟开源库，避免重复维护已有生态能力。

引入前必须核对候选的 API 与当前版本、维护状态、许可证、最低 Rust/平台版本、目标平台支持、性能特征和依赖体积，并在相关 Design Spec、Requirement 或研究记录中写明关键候选与适用性判断。只有当成熟方案不符合已对齐的领域语义、架构边界、性能或发布约束时才自行实现；此时应记录不采用原因，并把自研范围限制在 Picoo 特有的最小边界内。

已有自研实现不构成继续保留的理由。若成熟生态能以更低长期维护成本满足同一契约，应优先迁移并保留覆盖产品语义的回归测试。

## 外部 UI Skills（只 vendor，不改内容）

框架怎么写，用 `.agents/skills/` 里从上游拷来的 Skill。Picoo 长什么样，仍以 Design Specs 和 HTML 原型为准。不要装 `frontend-design` 或 Material Design 3 Skill。

| 场景 | Skill | 来源 |
| --- | --- | --- |
| GPUI Kit 框架、组件 API / Coding Guides | [gpui-kit](.agents/skills/gpui-kit/SKILL.md) | [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit) |
| GPUI Kit 官方 Design Guides | [gpui-kit-design-guides](.agents/skills/gpui-kit-design-guides/SKILL.md) | 同上 |
| Compose 状态与副作用 | [compose-state-and-effects](.agents/skills/compose-state-and-effects/SKILL.md) | [chrisbanes/skills](https://github.com/chrisbanes/skills) |
| Compose 组件 API | [compose-component-design](.agents/skills/compose-component-design/SKILL.md) | 同上 |
| Compose 动效 | [compose-animations](.agents/skills/compose-animations/SKILL.md) | 同上 |
| Compose UI 测试 | [compose-ui-testing-patterns](.agents/skills/compose-ui-testing-patterns/SKILL.md) | 同上 |
| Compose 自适应布局 | [adaptive](.agents/skills/adaptive/SKILL.md) | [android/skills](https://github.com/android/skills) |
| Compose 自定义 Styles（实验性） | [styles](.agents/skills/styles/SKILL.md) | 同上 |
| Android edge-to-edge | [edge-to-edge](.agents/skills/edge-to-edge/SKILL.md) | 同上 |
| Android 测试基础设施 | [testing-setup](.agents/skills/testing-setup/SKILL.md) | 同上 |
| 跨端 token 驱动 UI 代码 | [design-code](.agents/skills/design-code/SKILL.md) | [plugin87/ux-ui-agent-skills](https://github.com/plugin87/ux-ui-agent-skills) |

更新：常规 Skill 使用 `npx skills update`（锁文件 `skills-lock.json`）。`design-code` 的原始
Skill 与依赖闭包保存在 `.agents/vendor/ux-ui-agent-skills/`，入口仅处理 Codex 兼容路径；更新时按
`VENDOR.json` 替换完整上游快照，不要修改 vendor 内容。

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

## Cloud Agent 与跨平台构建

Cloud Agent 运行在 Linux 环境，**不能**在本机构建 Windows 桌面程序、MF 虚拟摄像头 DLL、macOS Camera Extension 或 iOS App。各平台最终二进制由 **GitHub Actions** 在对应 runner 上编译。

完整分工、runner 矩阵、workflow 约定与 Agent 工作流见 [docs/development/ci-and-build.md](docs/development/ci-and-build.md)。实现与修改 CI 时请遵循该文档，并与 [ARCH-PICOO-STACK-001](docs/design-specs/architecture/0001-rust-core-monorepo-boundary.md) 中的 xtask 边界一致。

### Agent 在 Cloud 中的职责

- Rust Core 开发、`cargo test`、协议测试与 `picoo-testkit` 模拟。
- Android Sender 构建（NDK + Gradle，可在 Linux 完成）。
- 维护 `.github/workflows/`，通过 `cargo xtask` 调用各平台构建，不在 workflow 中重复平台细节。
- 变更 push 后使用 **cursor-subscriptions** 的 `subscribe_github_ci` 等待 CI 结果，根据 Actions 日志迭代修复。

### 必须通过 GitHub Actions 构建的产物

| 平台 | Runner | 说明 |
| --- | --- | --- |
| Windows Receiver（GPUI + MF + VCam + 安装包） | `windows-latest` | 禁止在 Linux 上交叉编译整条 Receiver 链路 |
| macOS Receiver / Camera Extension | `macos-26` ARM64 + Xcode 26.6 | GPUI 编译基线已启用；Camera Extension 接入后扩展构建与签名 |
| iOS Sender | `macos-26` ARM64 + Xcode 26.6 | Rust XCFramework 基线已启用；SwiftUI App 接入后扩展构建与签名 |
| Android Sender | `ubuntu-latest` | Cloud 与 CI 均可构建 |

Android + Windows 已进入功能实现与产物验证；iOS + macOS 已进入平台构建基线与原生边界实现。Apple job 必须明确区分“Core/桌面可编译”和“App、Camera Extension、签名、真机链路已验证”，不得用前者替代后者的验收证据。

### CI 变更原则

- 新增平台构建时，先扩展 `xtask` 命令，再在 workflow 中增加对应 job。
- Windows/macOS/iOS 的签名与公证仅引用 GitHub Secrets，不在仓库中提交证书。
- CI 失败时优先阅读 Actions 日志；不要为通过 CI 而破坏 Architecture 边界（例如把 VCam 逻辑移入 Linux 可编译的 crate）。

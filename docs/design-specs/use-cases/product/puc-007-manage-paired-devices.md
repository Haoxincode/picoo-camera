# PUC-007：管理已配对设备与撤销信任

## 基本信息

- 类别：Product Device Trust Management
- 参与者：会议与录制用户
- 目标：在手机端和桌面端查看、删除已配对设备，并在撤销信任后强制重新配对

## 场景

用户在桌面端“连接”页的设备与连接区域查看已配对设备列表，包含设备名称、平台、最近连接时间和公钥指纹前缀。用户可删除某台已配对 Sender；删除后，该 Sender 下一次连接必须重新走 [PUC-001](puc-001-first-install-and-pairing.md) 的双端配对短码核对与确认流程。

用户在手机端也可删除已配对 Receiver。删除后，系统不再自动连接该 Receiver，且下一次连接必须重新配对。

同一 Sender 因应用数据重建而产生新加密身份时，Receiver 仍将其视为未配对身份并要求双端重新核对短码。未知新身份完成配对后，若本机存在同名历史可信身份，桌面端逐条显示其指纹并询问用户是否仅保留当前身份；用户确认后只撤销已展示的候选快照。已信任身份自动重连不提出清理决策。设备名称只用于提出清理候选，不构成身份连续性证明，也不得使新公钥继承旧信任。

用户可删除全部配对关系和本地配置；日志中的 IP、设备名和公钥指纹应支持脱敏。系统不向公网发送设备名称、局域网信息或运行指标。

## 可观察结果

- 手机端和桌面端均提供已配对设备管理入口。
- 删除配对后，对方设备不能在没有重新确认的情况下恢复视频传输或驱动虚拟摄像头。
- 公钥变化时，系统拒绝将其视为同一已配对设备。
- 新身份完成配对后可一次撤销同名历史身份；取消替换时所有独立信任关系保持不变。
- 用户可以导出诊断信息，但默认不写入任何视频文件；日志不包含视频帧。

## 边界

- 配对关系仅保存在本地，不提供跨设备云同步。
- 不读取 Android ID、IMEI、MAC 地址或其他平台硬件标识来推断物理设备身份。
- 本 Use Case 不包含组织级设备管理、MDM 或批量撤销。
- 删除配对不会自动卸载虚拟摄像头组件；虚拟摄像头修复属于桌面端设置能力，但不是本 Use Case 的核心目标。

## 相关 Architecture

- [ARCH-PICOO-DISCOVERY-001](../../architecture/0008-discovery-and-pairing-security-boundary.md)
- [ARCH-PICOO-STACK-001](../../architecture/0001-rust-core-monorepo-boundary.md)
- [ARCH-PICOO-UI-001](../../architecture/0009-desktop-gpui-mobile-native-ui-boundary.md)
- [ARCH-PICOO-UI-002](../../architecture/0010-cross-platform-design-system-boundary.md)

## 相关 Requirements

- [REQ-PICOO-PAIRING-005/006](../../requirements/pairing.md)
- 待分解：`REQ-PICOO-PRIVACY-*`

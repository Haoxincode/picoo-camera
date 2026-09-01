# ARCH-PICOO-VCAM-001: 虚拟摄像头平台边界

Status: planned
Source: product PRD V1.0 / PUC-001 / PUC-004

## 背景

会议软件通过操作系统标准摄像头 API 枚举设备。Picoo Camera 必须在 Windows 与 macOS 上注册统一名称 **`Picoo Camera`**，并向 Zoom、Teams、腾讯会议、OBS 和浏览器会议提供稳定 NV12 帧流。

Windows 与 macOS 的虚拟摄像头机制不同，但产品语义一致：无连接占位、有连接实时帧、扩展进程不持有网络逻辑。

## 架构决策

### 统一产品名称

Windows 与 macOS 都向系统注册：**Picoo Camera**

### Windows

使用 Media Foundation Virtual Camera API（最低 Windows 11 Build 22000）：

```text
MFCreateVirtualCamera
  → IMFVirtualCamera
  → Custom IMFMediaSource (PicooVirtualCameraSource.dll)
  → Shared Frame Ring
```

组件包括：

- `Picoo Camera Desktop.exe`
- `PicooVirtualCameraSource.dll`
- Installer
- Shared Frame Ring

Media Source 作为独立组件安装并注册，由 Windows Frame Server 加载。

Windows 输出媒体类型由 Frame Server 客户端在 `Start` 的 presentation descriptor 中选择，
并在该次运行周期内保持稳定。Shared Frame Ring 的 producer 分辨率不是重新协商信号；MF Source
必须在自身边界把输入 NV12 等比缩放并以黑边补齐到已协商的 480p/720p/1080p。不得在
`RequestSample` 中因占位帧、直播帧或方向变化而反向修改 current media type 或重建 allocator。
转换后的直播帧和占位帧必须按源帧 revision 与输出尺寸复用，像素转换不得持有 stream 的 COM
状态锁。默认 allocator 只允许在 stream stopped 状态替换；stream start/stop 的 allocator、事件与
可见状态必须作为事务提交，失败时不得留下半启动状态。`RequestSample` 的像素阶段完成后必须按
lifecycle revision 复核，并与 start/stop/shutdown 共用 lifecycle operation 锁后才可访问 allocator
和提交 `MEMediaSample`，防止停流事件被旧请求反向越过。

`PicooVirtualCameraSource.dll` 是 Windows-only Rust `cdylib`，使用 `windows-rs` 的类型化 Win32/COM 绑定实现 `IClassFactory`、`IMFActivate`、`IMFMediaSourceEx`、`IMFMediaStream2`、`IMFGetService`、`IKsControl` 与 `IMFSampleAllocatorControl`。它由 Cargo 在 Windows runner 上构建，不维护 C++、WRL、VCXPROJ 或 MSBuild 项目。DLL 可以复用 `picoo-frame-hub` 的 Shared Frame Ring 布局与占位帧实现，但不得依赖 Receiver、QUIC、解码或配对 crate。

AllUsers + System-lifetime 摄像头由 per-machine MSI 或用户显式 UAC 修复时创建；普通桌面启动不得再创建一个重名的 Session-lifetime 摄像头。安装与卸载维护命令必须在非 impersonated 的管理员上下文中执行，使多用户枚举、静默部署、升级与清理使用同一身份。Windows 会在产品提供的 base friendly name 后追加并本地化 `Windows Virtual Camera` 标识，因此状态不得把显示名精确等于 `Picoo Camera` 作为身份条件；只有 Media Foundation 枚举到安装时持久化的精确 symbolic link 才能提升为 Active，DLL、COM 注册表存在或任意同名设备都不能冒充已注册成功。`IMFVirtualCamera::Start` 成功后必须取得并持久化 `MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK`，并以有界重试等待 Software Device 发布到 Media Foundation，不能用一次立即枚举判定失败；仅当精确身份在操作前不存在时，Start/持久化失败才能 best-effort Remove，repair 不得删除操作前已存在的设备。卸载 Remove 不依赖 Source Start；identity 不存在时是幂等成功。维护进程必须先调用 `IMFVirtualCamera::Shutdown` 并 Release camera，再执行 `MFShutdown` 与 `CoUninitialize`；任何返回路径都不得颠倒该所有权顺序。成功 Remove 后的动态 identity 清理是 best-effort，声明式 COM 由 MSI 自身事务删除。

Windows major upgrade 是独立事务边界。仍受支持的旧 MSI 可能在 `RemoveExistingProducts` 中执行其已发布且不可修改的卸载命令，因此升级包必须先以单调递增的 PE FileVersion 安装新版维护程序，再移除旧产品，最后重新注册摄像头；成功事务结束时只允许保留新 ProductCode 与单一 exact identity。新包的升级卸载条件必须排除 `UPGRADINGPRODUCTCODE`，防止该桥接成为常态。新注册失败时，rollback Custom Action 必须在文件回滚前 best-effort 恢复旧产品可用的 identity；全新安装仍使用反向 Remove 回滚。组件 GUID、KeyPath 与安装目录在 late upgrade 下必须保持严格稳定，CI 直接查询 MSI 数据库验证动作顺序和 FileVersion，真机仍须验证受支持的旧版到新版升级。

### macOS

使用 Core Media I/O Camera Extension（API 最低 macOS 12.3；产品基线 macOS 15 ARM64）：

```text
Picoo Camera Desktop.app
  → App Group Container
  → mmap Shared Frame Ring
  → Picoo Camera Extension.systemextension
```

Camera Extension 作为桌面应用随附的系统扩展，首次使用时由用户批准。扩展是独立进程边界；主应用不得把网络会话逻辑放入扩展。

当前原生基线使用 Swift 6、Core Media I/O 和 C17 原子共享环读取边界，提供 480p/720p/1080p、30 fps、NV12 输出。`xtask package macos` 在 ARM64 macOS runner 上生成 `Picoo Camera.app`，并将 Camera Extension 嵌入 `Contents/Library/SystemExtensions/`；打包门禁校验 Host/Extension Bundle ID、显式注册的 `group.com.haoxincode.picoo-camera` App Group、Host sandbox/network/System Extension 签名输入、同步版本号与 ARM64 slice。Host 通过纯 Rust `objc2-system-extensions` 适配官方 SystemExtensions 框架，后台执行 properties、activation 与 deactivation request，保留弱 delegate 的完整生命周期，仅允许更高 `CFBundleVersion` 替换，并把重启后完成的激活/停用意图连同 `kern.boottime` 启动会话持久化；若系统已重启但 properties 仍未收敛，必须清除 pending 锁、展示失败并允许重试。只有系统 properties/完成回调可以把设备标为 Active，检测中和视频会话本身均不得提升虚拟摄像头状态。无签名构建使用明确的 `UNSIGNED.` Team 前缀，并在 Host Info.plist 写入独立的 unsigned development marker；Host 只根据该 marker 将共享环降级到用户 Application Support 目录，不从正式 App Group 字符串推断签名状态，从而避免 LaunchServices 在无有效 entitlement 时阻塞启动。该降级不作为扩展互通验收。发布构建由 `xtask release macos` 校验 Developer ID profile 的有效期、分发类型、授权证书与 capability，并直接使用已验证的证书 SHA-1 指纹从内到外签名，再复核实际 Team、Authority 与 effective entitlements，最后 notarize 与 staple。用户批准、重启后的系统枚举、实际签名凭据绿测和会议软件枚举仍属于真机验收，因此本 Architecture 保持 `planned`。

### 数据流

```text
Rust Receiver Core
  → Decode once
  → FrameHub
  → Shared Frame Ring
  → Virtual Camera Extension / MF Media Source
  → Zoom / Teams / 腾讯会议 / OBS / Browser
```

### 安装与修复

- Windows 安装器：注册 COM/Media Foundation 组件、配置防火墙规则、卸载清理。per-machine 安装与显式 UAC 修复统一创建 AllUsers 设备，不把设备可见性绑定到执行安装的某个账户。
- macOS 发布：签名、Hardened Runtime、Developer ID、Notarization、扩展激活引导。
- 桌面“虚拟摄像头”页提供状态检查与修复入口；Windows 显式修复通过 UAC 提权的独立维护进程写系统注册，GPUI 进程只负责发起、等待与展示结果。

## 不采用的方案

### macOS DAL 插件

不采用。Camera Extension 是 Apple 推荐的现代系统扩展机制。

### 在虚拟摄像头进程内运行 QUIC 或解码

不采用。扩展/Media Source 只读 Shared Frame Ring。

### 为 Windows Media Source 维护 C++/WRL 工程

不采用。COM 与 Media Foundation 接口由 `windows-rs` 提供类型化 ABI 和实现宏；继续维护等价的 C++/WRL、共享环结构体副本和独立 MSBuild 工程会扩大工具链与跨语言一致性风险。

### Linux v4l2loopback 第一版

不采用。第一版只覆盖 Windows 与 macOS Receiver。

## 约束

- 未配对或未连接时输出定义占位画面，不是不可枚举设备或随机噪声。
- 会议软件关闭并重新打开后仍可选择 Picoo Camera。
- 虚拟摄像头组件升级必须与 Desktop 主应用版本兼容，并通过安装器或应用内修复流程处理。
- Rust COM DLL 的公开 ABI 不得让 panic 越过导出函数或 COM vtable；共享可变状态必须显式同步，DLL 仅在 Windows runner 上完成最终链接与加载验证。
- Windows Media Foundation 组件必须满足 free-threaded/neutral 约束：对外实现 `IAgileObject`，其余共享状态由互斥锁串行化。Media Foundation event queue、descriptor、allocator 等标准接口直接持有类型化 COM 引用；不得对这些接口调用 `RoGetAgileReference`，因为 Frame Server 环境中并不保证其 IID 存在代理注册，错误包装会让 Source 激活在注册阶段以 `REGDB_E_IIDNOTREG` 失败。

## 相关 Use Case

- [PUC-001](../use-cases/product/puc-001-first-install-and-pairing.md)
- [PUC-004](../use-cases/product/puc-004-use-virtual-camera-in-meeting-apps.md)

## 相关 Architecture

- [ARCH-PICOO-FRAME-001](0006-framehub-shared-frame-ring-boundary.md)
- [ARCH-PICOO-STACK-001](0001-rust-core-monorepo-boundary.md)

## 相关 Requirements

- [REQ-PICOO-VCAM-001..009](../requirements/vcam.md)

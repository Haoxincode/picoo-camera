# Apple 原生图像与 GPU 转换选型

关联 ARCH-PICOO-MEDIA-002、REQ-PICOO-NEXT-009/016/029。日期：2026-09-06。

## 当前接口与边界

锁定的 GPUI Kit 0.6 / gpui-pre-apple 0.3.3 在 Metal renderer 中只接受 `420f`，shader 使用固定 BT.601 full-range 矩阵。不能把 Decoder 的 BT.709 limited NV12 直接交给 surface。窗口只持有已准备图像；方向、缩放及颜色处理由独立 GPU 工作者拥有。

原生图像复用官方 CoreVideo/IOSurface retain/release，不自行分配公共 CPU 像素，也不自制平台引用计数。Rust 使用仓库已有系列的 objc2-core-foundation/core-video 0.3.2，以及同系列 objc2-io-surface 0.3.2；生成绑定来自维护中的 objc2，许可证 Zlib/Apache-2.0/MIT 可选，最低 Rust 1.71，低于项目当前工具链。它们只链接 Apple 系统 framework；按 macOS target 和所需 features 启用，不为其他目标增加框架依赖或软件渲染器。启用 CoreVideo 的 IOSurface API 时必须显式启用 IOSurfaceRef，否则最小 features 构建缺少 CF Type 实现。

## GPU 转换候选

- 官方 Core Image + 显式 Metal device：系统维护 NV12 色彩转换、仿射变换及缩放；使用 `CIRenderDestination(pixelBuffer:)` 与 `CIRenderTask.waitUntilCompleted()` 获得完成边界。CIRenderTask 自 macOS 10.13 可用，满足当前 macOS 26.6 构建基线。Rust 同系列 objc2-core-image 0.3.2 已提供实际生成方法签名。系统实现不引入软件 codec 或额外动态库分发；最终 feature/包体增量仍需在接入后量测。
- 自写 Metal shader：能精确控制 plane 采样和转换，但需长期维护色彩、滤波、方向组合及 GPU 测试。在官方接口满足契约前不选择这条路。
- wgpu：跨平台命令封装成熟，但不能直接替代平台 NV12/IOSurface/CMIO 资源所有权，仍需原生 interop；此处不因统一语法引入第二套 GPU device 生命周期。

优先验证 Core Image。该判断不代表已完成生产接入，也不保证所有源色彩或驱动行为。GPU 任务完成等待只能在有界专用工作者上；任务仍在执行时不能因超时丢弃最后一份输入/输出 lease 或复用资源。

## M4 实测

`swift verification/native-media/core-image-color-probe.swift` 在 Apple M4/macOS 26.6.2、Xcode 26.6 成功。显式 Metal device，关闭软件 renderer，对 1280×720 红色生成图渲染到 IOSurface `420f`：

| 目标 attachment | 实际 Y/U/V | Core Image pass |
| --- | --- | --- |
| BT.601 | 76/85/255 | 1 |
| BT.709 | 54/99/255 | 1 |

说明当前系统尊重目标 matrix，提供 GPUI 所需转换的可行性证据。probe 的 CPU 锁定只用于完成后的诊断读值，不是生产图像链路。仍需真实 Decoder 输入、黑白/色条、transfer/primaries、旋转镜像、池容量和 GPU 完成/取消测试；单色结果不作为完整画质或实时性能验收。

## Rust GPU 边界验证

`picoo-gpu` 通过 objc2 0.6.4 与 framework 0.3.2 接入上述实际 API。只增加已有锁定绑定的必要 features，无新版本包或 shader 编译工具链。context 关闭软件 renderer 和中间图像缓存，每次渲染使用 autorelease pool；三张 IOSurface 输出通过 CVPixelBufferPool allocation threshold 限制，调用者持有 clone 时不重复使用该 allocation。`CIRenderTask` 完成等待在 renderer 的专用调用线程内，返回的是完成图像，不把提交当作完成。

实际灰阶测试发现，CVPixelBuffer 的 matrix/primaries/transfer attachments 不足以让 Core Image 自动推断出与指定目标完全相同的传递曲线：原 Y=40 被改变为 52。生产适配现明确传入 `kCIImageColorSpace = CGColorSpaceITUR_709`，并分别指定 BT.709 输出或 GPUI sRGB 输出色彩空间。BT.709 源 attachment 缺失或不匹配时拒绝，不修改共享源对象、不猜测。修改后灰阶、四方向 × 镜像、contain 黑边和两种颜色输出测试通过。

当前验证是合成原生 NV12 经真实 Metal/Core Image 的图像和资源测试；Decoder、FrameBus、预览 UI 尚未改用它。GPUI 后续 GPU 读取的 CVMetalTexture/CVPixelBuffer 保留与完成交接仍需单独审查，不能用本 renderer 已完成写入代替下游 GPU 读取完成。固定布局池的局部上限也不代替跨输出、重建和资源代际的总预算。

# Windows 原生图像与 GPU 边界

关联 ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-009、016、029，2026-09-06。

现有 GPUI Kit 锁定的 gpui-pre-windows 0.3.3 使用 D3D11；directx_renderer.rs 的 draw_surfaces 当前只返回 Ok，不提供视频表面绘制。因此不能把 macOS surface API 直接视为 Windows 已实现。框架资源适配只能拥有纹理导入、GPU 读取寿命和绘制，不接收 Picoo 配置或 codec 事务。

原生图像选择官方 D3D11 + Media Foundation 的 IMFDXGIBuffer，而不是把 GPU 图像映射到 CPU 再上传。复用仓库已有 windows-rs 0.62.2，MIT OR Apache-2.0，最低 Rust 1.82；项目当前 stable 高于基线。API 核对当前绑定中的 IMFDXGIBuffer::GetResource/GetSubresourceIndex、ID3D11Texture2D::GetDesc、MFCreateDXGISurfaceBuffer；均早于 Windows 11 项目最低平台。增加的 feature 是已被 MF Decoder 依赖树采用的官方类型投影，不引入 C/C++ 编译器、FFmpeg 或另一套 GPU runtime。

候选 wgpu 面向跨后端 GPU 工作，但本项目 Windows codec 与 GPUI 都使用 D3D11，引入 D3D12/Vulkan 资源互操作会增加同步与 adapter 边界。此处不采用；D3D11 VideoProcessor/共享资源是后续目标图像处理与原生交接候选，仍须逐项查询硬件支持。

解码图像 owner 必须保留原始 IMFSample 和纹理/subresource；仅保留 ID3D11Texture2D 不足以阻止 MF allocator 将数组 slice 重新交给解码器。构造入口为 unsafe 已完成且不可变的输出边界；安全接口仅公开几何，平台 GPU 访问同样要求持有 owner 到 GPU 完成。该边界不提供 CPU map、像素或 stride，不替代输出 GPU 完成 fence。

Windows CI 的 WARP 只可用于标准资源对象、COM 保留和边界拒绝的诊断测试，不作为硬件解码、显卡矩阵、吞吐或生产设备工厂验收。生产链路仍必须拒绝软件 codec/GPU。

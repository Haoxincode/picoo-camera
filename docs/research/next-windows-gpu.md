# Windows 原生图像与 GPU 边界

关联 ARCH-PICOO-MEDIA-002 / REQ-PICOO-NEXT-009、016、029，2026-09-06。

现有 GPUI Kit 锁定的 gpui-pre-windows 0.3.3 使用 D3D11；directx_renderer.rs 的 draw_surfaces 当前只返回 Ok，不提供视频表面绘制。因此不能把 macOS surface API 直接视为 Windows 已实现。框架资源适配只能拥有纹理导入、GPU 读取寿命和绘制，不接收 Picoo 配置或 codec 事务。

原生图像选择官方 D3D11 + Media Foundation 的 IMFDXGIBuffer，而不是把 GPU 图像映射到 CPU 再上传。复用仓库已有 windows-rs 0.62.2，MIT OR Apache-2.0，最低 Rust 1.82；项目当前 stable 高于基线。API 核对当前绑定中的 IMFDXGIBuffer::GetResource/GetSubresourceIndex、ID3D11Texture2D::GetDesc、MFCreateDXGISurfaceBuffer；均早于 Windows 11 项目最低平台。增加的 feature 是已被 MF Decoder 依赖树采用的官方类型投影，不引入 C/C++ 编译器、FFmpeg 或另一套 GPU runtime。

候选 wgpu 面向跨后端 GPU 工作，但本项目 Windows codec 与 GPUI 都使用 D3D11，引入 D3D12/Vulkan 资源互操作会增加同步与 adapter 边界。此处不采用；D3D11 VideoProcessor/共享资源是后续目标图像处理与原生交接候选，仍须逐项查询硬件支持。

解码图像 owner 必须保留原始 IMFSample 和纹理/subresource；仅保留 ID3D11Texture2D 不足以阻止 MF allocator 将数组 slice 重新交给解码器。构造入口为 unsafe 已完成且不可变的输出边界；安全接口仅公开几何，平台 GPU 访问同样要求持有 owner 到 GPU 完成。该边界不提供 CPU map、像素或 stride，不替代输出 GPU 完成 fence。

Windows CI 的 WARP 只可用于标准资源对象、COM 保留和边界拒绝的诊断测试，不作为硬件解码、显卡矩阵、吞吐或生产设备工厂验收。生产链路仍必须拒绝软件 codec/GPU。

GPU context 采用官方 DXGI adapter、D3D11CreateDevice、ID3D11Multithread 和 MFCreateDXGIDeviceManager/ResetDevice。创建时固定关联一个 device，只在初始化时 ResetDevice；对外原生访问不得重新绑定 manager。MF/COM runtime 由平台 codec 工作者管理，不在可跨线程图像/context 的 Drop 中 CoUninitialize。context 不是某 codec 的硬件能力证明，生产设备入口先拒绝 DXGI_ADAPTER_FLAG_SOFTWARE；WARP 明确排除。公开 for_adapter 接口允许后续平台预览与解码选择同一 adapter，不用新增 sink 触发整个源 device 重建。


## MFT 硬件模式准入

微软 [AVDecVideoAcceleration_H264](https://learn.microsoft.com/en-us/windows/win32/codecapi/avdecvideoacceleration-h264-property) 明确说明该属性在 Media Foundation/IMFTransform 路径无效，不能通过 SetValue(true) 声明已开启硬解。

官方 [D3D11 MF 解码接入](https://learn.microsoft.com/en-us/windows/win32/medfound/supporting-direct3d-11-video-decoding-in-media-foundation) 要求先检查 MF_SA_D3D11_AWARE，再通过 MFT_MESSAGE_SET_D3D_MANAGER 传入 DXGI manager。驱动 profile、NV12 output format 与完整 decoder configuration 由 D3D11 VideoDevice 查询；输入/输出类型必须在 manager 已绑定时协商。

文档规定不支持硬件组合时 SetInputType/SetOutputType 返回 MF_E_UNSUPPORTED_D3D_TYPE。标准 Topology Loader 的软件回退是发送 SET_D3D_MANAGER(NULL)，然后重新协商。Picoo 直接管理 MFT，因此必须把此错误作为明确准入失败，禁止清空 manager 后重试。完成帧还必须是同一设备的合法 IMFDXGIBuffer；普通 IMFMediaBuffer 不能进入原生源。MFT 的包装器是否被称作 software decoder 或是否注册为异步 hardware MFT，不足以代替这些实际契约；不能只凭 factory 标签宣称或否定 DXVA。

参考微软 [H.264 Decoder](https://learn.microsoft.com/en-us/windows/win32/medfound/h-264-video-decoder)：DXVA 支持 Main-compatible Baseline/Main/High，1920×1088 为其说明的保证尺寸上限。Picoo 仍按当前 codec/profile/实际 coded size/驱动能力验证，不能将该文档替代所有显卡与 HEVC 的验收。

生产工厂接入复用已有 WindowsGpuContext，并使用官方 ID3D11VideoDevice::CheckVideoDecoderFormat/GetVideoDecoderConfigCount 对 H264_VLD_NOFGT + NV12 + coded size 查询驱动配置。它不提供 60fps 热稳态吞吐保证，MFT SetInputType/SetOutputType 与实际输出仍是独立准入。GetResource/GetDevice/IUnknown 检查实际 sample 所属设备，CPU sample 或另一个 device 均拒绝。首个非 software adapter 的初始化失败直接报告，不为新增 sink 或运行故障重选 source adapter。

GPU 完成 API 进一步核对了官方 ID3D11DeviceContext4::Signal、ID3D11Fence::GetCompletedValue/SetEventOnCompletion：Signal 只允许 immediate context，完成值覆盖此前工作，但 HRESULT 失败不构成已完成证据。尚未将未处理 signal/注册失败寿命的通用 callback tracker 加入产品；资源保留与失败清理须先闭合，再替换源 CPU 读取。

## GPU 完成事件

复用当前 windows-rs 的 [ID3D11DeviceContext3::Flush1](https://learn.microsoft.com/en-us/windows/win32/api/d3d11_3/nf-d3d11_3-id3d11devicecontext3-flush1) 与 D3D11_CONTEXT_TYPE_ALL：官方支持传入 Win32 event 建立异步完成查询，返回 void，不需要应用自行维护 Signal 值。Windows 11 产品基线满足 D3D11.3/4；接口查询在提交前失败则明确拒绝，不回退轮询。每工作独立事件，避免复用 fence/event 时的世代混淆。

[RegisterDeviceRemovedEvent](https://learn.microsoft.com/en-us/windows/win32/api/d3d11_4/nf-d3d11_4-id3d11device4-registerdeviceremovedevent) 可使用同一事件，已移除设备会立即置位；回调检查 GetDeviceRemovedReason，不能把移除误认作成功完成。事件注销先于句柄释放。复用官方 CreateThreadpoolWait/SetThreadpoolWait；wait 为一次性，不重新 arm，CloseThreadpoolWait 可在自己的完成回调内异步清理，不在回调中等待自身结束。泛型 owner 的析构或通知异常不能穿越系统 callback ABI。

## 原生几何与颜色处理

复用 ID3D11VideoDevice/VideoContext1、VideoProcessorEnumerator1 的格式转换查询。官方 [VideoProcessorSetStreamMirror](https://learn.microsoft.com/en-us/windows/win32/api/d3d11_1/nf-d3d11_1-id3d11videocontext1-videoprocessorsetstreammirror) 规定 rotation、mirror、source clipping 的顺序；不能把未经转换的 SPS/native crop 直接放入旋转后的 source rect。成熟平台接口覆盖缩放、旋转、镜像和颜色，不引入自研 YUV shader 或另一套 GPU runtime。Windows 输出先采用可精确表达的 DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709，不把 Apple 现有混合标签的 Bt601Full 直接映射成 DXGI P601 并宣称等价。驱动缺少正式颜色或变换能力时明确拒绝。

## 输出专用 CPU 导出

复用 D3D11 CopyResource、staging texture 与 Map/Unmap，使用现有完成事件。官方 [DXGI_FORMAT_NV12](https://learn.microsoft.com/en-us/windows/win32/api/dxgiformat/ne-dxgiformat-dxgi_format) 规定 staging/initData 的长度为 rowPitch × (height + height/2)，Y 平面为前 rowPitch × height，UV 为余下行；两者行 pitch 一致，宽高必须为偶数。导出只复制目标有效 width，不把 padding 当像素。

官方 [D3D11_MAP_FLAG_DO_NOT_WAIT](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/ne-d3d11-d3d11_map_flag) 使仍被 GPU 占用的资源返回 DXGI_ERROR_WAS_STILL_DRAWING；READ mapping 支持该标志。使用前已等待完成事件，若它仍报告 busy 则明确失败，不增加轮询或隐藏等待。三槽 CPU 输出池直接提取自现有 Apple 实现并由两端复用，无新增通用池库、像素转换库或源 CPU 接口。

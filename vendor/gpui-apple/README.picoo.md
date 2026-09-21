# GPUI Apple surface 生命周期修补

关联 ARCH-PICOO-MEDIA-002、REQ-PICOO-NEXT-016。来源、固定 release checksum 与许可证见 ORIGIN.json / LICENSE-APACHE。这是现有 gpui-pre-apple 0.3.3 的局部依赖修补，Cargo 根 manifest 通过 patch 选择它；没有引入另一套桌面框架。

上游 draw_surfaces 创建的 CVMetalTexture 包装器在编码完 draw 后即释放；Metal 后续读取仍可能进行。CoreVideo 要求源 CVPixelBuffer 和 CVMetalTexture 保留到 GPU 完成，不能仅依赖 MTLTexture 的引用，也不能以若干帧延迟推测完成。现在每条 command buffer 保留实际使用的三元 lease，并在完成回调中释放；Scene 与 Preview 可以提前释放自身引用，输出池仍不能复用 GPU 正在读取的 allocation。

原 1626 行 metal_renderer 按实际职责拆为 renderer 生命周期、primitive 编码/pipeline 构建、instance buffer 分配三个模块；三个实现文件均低于 800 行。vendor/gpui/src 是发布包原有 cbindgen 输入快照，用于生成 Metal ABI，不作为 Picoo 应用实现扩写。Metal shader 仅清理七行缩进中的空格后 Tab，不改变 shader 运算。除列出的修补和模块拆分外，保留发布包源文件与框架 API；不修改 vendored UI Skills。

升级时下载官方 release，核对 checksum / LICENSE，检查其是否已保留这两类 CoreVideo 资源。如果已修复，运行阻塞 GPU 读取的 pool 复用回归和桌面测试后移除本 patch；否则在新 release 上重新应用同一局部契约，不叠加 Picoo 产品行为到框架。

本地资源回归构建：

```sh
cargo test -p picoo-desktop -p gpui-pre-apple --features picoo-desktop/gpui-ui --lib --no-run
```

运行编译得到的 gpui_apple 单元测试二进制中的 `stalled_surface_read_holds_pool_allocation_until_metal_completion`。Mac 大型 Cargo deps 目录会触发首次 IOSurface 初始化时的 NSBundle 扫描；验证时把测试可执行文件复制到独立目录，保持测试期限和 GPU 完成条件不变。

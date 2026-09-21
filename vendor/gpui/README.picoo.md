# GPUI Windows 原生图像接入

关联 ARCH-PICOO-MEDIA-002、REQ-PICOO-NEXT-016 与 REQ-PICOO-GPU-008。发布包版本、校验和及许可证见 ORIGIN.json 和 LICENSE-APACHE。保持现有 GPUI 依赖版本；根 Cargo.toml 使用局部 patch。

GPUI 的 Windows surface 原先没有绘制实现。局部扩展接受外部 BGRA8 shader view，复用现有 PolychromeSprite shader、布局与裁剪。资源提供者负责跨设备导入、访问锁及 GPU 完成前的所有权；框架不依赖 Picoo，不引入第二套完成调度器，也不上传 CPU 像素。绘制结束解除 shader 资源绑定，忙碌或失败图像不阻止其他 UI 重绘。

新增职责位于独立 surface 模块。升级时与官方发布包核对局部差异；上游满足同一资源契约后运行原生显示与生命周期测试再移除 patch。不得将只编译成功记录为实际 Windows 预览通过。

Windows 原生回归由 `cargo xtask test windows` 调用，也可从根 workspace 单独运行：

```sh
cargo test -p gpui-pre-windows -p picoo-desktop --features picoo-desktop/gpui-ui,picoo-desktop/windows-vcam,gpui-kit/test-support --lib native_surface_shader_draws_bgra_and_respects_clip
```

测试使用 WARP 诊断 device 验证真实 shader、像素裁剪、资源解绑和 view 身份；不代表实际硬件 Video Processor 颜色或完整 Receiver 预览验收。

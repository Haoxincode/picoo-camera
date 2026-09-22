# REQ-PICOO-PRIVACY：隐私与本地-only

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-PRIVACY-001 | implemented | PUC-007 | 不向公网发送设备信息或指标 | 无 HTTP/遥测客户端；指标仅 PCP 本地 `ReceiverStats` |
| REQ-PICOO-PRIVACY-002 | implemented | PUC-007 | 默认不写入视频文件 | 帧仅内存/Shared Frame Ring；诊断 `includes_video: false` |
| REQ-PICOO-PRIVACY-003 | implemented | PUC-007 / ARCH-PICOO-UI-001 | 日志不含视频帧；IP/设备名/指纹可脱敏；桌面端可直接定位已导出的诊断文件；GUI 导出包含构建号（存在时）与独立预览诊断，区分收帧准入、显示需求、准备、交付和 Windows 纹理读取/绘制提交；固定阶段计数、最近进展时间与有界原生错误不持有像素、纹理或设备身份；未观测绘制的平台必须明确标记；提交绘制不等于屏幕显示或 GPU 完成 | `picoo-diagnostics` 默认脱敏；session 计数 role-neutral；Android/桌面导出；`preview` 测试覆盖阶段停顿、busy、迟到代际、并发计数和错误长度；`diagnostics_export` 验证 preview 与脱敏报告同文件输出；Windows 原生绘制计数与灰屏真机定位待验证；[采集与判读](../../development/windows-preview-diagnostics.md) |
| REQ-PICOO-PRIVACY-004 | implemented | PRD §21 | QUIC/TLS 加密，抓包无明文控制/视频 | 控制+视频经 Quinn/Rustls ALPN `picoocam`；mDNS 广播仅含公开白名单字段 |
| REQ-PICOO-PRIVACY-005 | implemented | PUC-007 | 断网外网后 LAN 功能仍可用 | 无云依赖；`paired_loopback_binds_lan_only_without_wan` + 架构无 HTTP 遥测 |

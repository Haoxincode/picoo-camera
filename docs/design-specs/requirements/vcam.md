# REQ-PICOO-VCAM：Windows 虚拟摄像头

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-VCAM-001 | implemented | PUC-004 | 注册统一名称 `Picoo Camera` | `FRIENDLY_NAME` / `PICOO_VCAM_FRIENDLY_NAME`；真机枚举仍待 |
| REQ-PICOO-VCAM-002 | implemented | ARCH-PICOO-VCAM-001 | MFCreateVirtualCamera + 独立 IMFMediaSource DLL | Session 生命周期由桌面进程持有；`MENewStream`/`MEUpdatedStream`；720p+1080p NV12 |
| REQ-PICOO-VCAM-003 | implemented | PUC-004 | VCam 进程只读 Shared Frame Ring，不持有 QUIC/解码器 | `picoo_frame_provider` / `ring_reader` 仅映射共享环 |
| REQ-PICOO-VCAM-004 | proposed | PUC-001 | 安装器注册 COM/MF 组件与卸载清理 | MSI `regsvr32`；MF Start 在首次桌面启动；安装测试待 CI/真机 |
| REQ-PICOO-VCAM-005 | proposed | PUC-004 | Zoom/Teams/腾讯会议/OBS/浏览器可选用 | 兼容 checklist（需 Win11） |

# REQ-PICOO-VCAM：Windows 虚拟摄像头

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-VCAM-001 | proposed | PUC-004 | 注册统一名称 `Picoo Camera` | 系统设备枚举 |
| REQ-PICOO-VCAM-002 | proposed | ARCH-PICOO-VCAM-001 | MFCreateVirtualCamera + 独立 IMFMediaSource DLL | DLL 注册成功 |
| REQ-PICOO-VCAM-003 | proposed | PUC-004 | VCam 进程只读 Shared Frame Ring，不持有 QUIC/解码器 | 架构审查+运行时验证 |
| REQ-PICOO-VCAM-004 | proposed | PUC-001 | 安装器注册 COM/MF 组件与卸载清理 | MSI 安装测试 |
| REQ-PICOO-VCAM-005 | proposed | PUC-004 | Zoom/Teams/腾讯会议/OBS/浏览器可选用 | 兼容 checklist |

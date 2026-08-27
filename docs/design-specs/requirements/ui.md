# REQ-PICOO-UI：Android Compose + Windows GPUI

| ID | 状态 | 来源 | 描述 | 验收 |
| --- | --- | --- | --- | --- |
| REQ-PICOO-UI-001 | proposed | ARCH-PICOO-UI-001 | GPUI View 不直接持有 QUIC/Decoder/Frame Buffer | DesktopAppState 事件驱动 |
| REQ-PICOO-UI-002 | proposed | PRD §16 | 桌面四页：首次启动/等待连接(含QR)/直播/设置 | 页面导航测试 |
| REQ-PICOO-UI-003 | proposed | PRD §17 | 手机三页：设备列表/配对/传输 | Compose 导航测试 |
| REQ-PICOO-UI-004 | proposed | ARCH-PICOO-UI-001 | VideoSurface 只渲染纹理，不拥有解码器 | 组件边界审查 |
| REQ-PICOO-UI-005 | proposed | PUC-005 | Sender 前台、防锁屏、过热/低电量提示 | Android 真机测试 |
| REQ-PICOO-UI-006 | implemented | PUC-001 | 权限在操作时请求，非启动一次性弹出 | Enable camera / Scan QR 时请求 CAMERA |

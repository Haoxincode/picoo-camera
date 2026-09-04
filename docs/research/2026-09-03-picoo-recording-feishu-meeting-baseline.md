# Picoo Recording：飞书会议高质量录制成片研究基线

日期：2026-09-03

状态：Research，非规范性

范围：飞书负责实时会议；Picoo Camera 负责摄像头；Picoo Recording 负责独立素材轨与会后成片

> 本文保存后续需求讨论的产品研究基线，只保留核心场景、目标和边界。
> 它不直接定义产品行为，也不替代当前 [产品基线 V1.0](../product/picoo-camera-prd-v1.0-2026-08-27.md)、Use Case、Architecture 或 Requirement。
> 已确认的产品决策仍应进入对应产品基线与 Design Specs；实现不得只引用本文作为验收依据。

## 1. 研究问题

本次记录要回答的不是实现细节，而是产品定位是否成立：

1. 团队继续使用飞书开会时，Picoo 是否应该额外承担高质量录制与会后成片，而不是替代飞书？
2. Picoo Camera、Picoo Recording 与飞书三者的责任应如何划分？
3. 高质量成片依赖哪些素材轨，这些轨是否必须全部来自 Picoo 手机端？
4. 当前 V1.0 明确不做的本地录像、多机位、4K、抠图，与这条产品线是什么关系？

## 2. 背景

团队目前仍然希望使用 **飞书作为主要会议工具**。

飞书已经很好地解决了：

- 会议邀请与成员管理
- 实时语音和视频
- 屏幕共享
- 聊天与会议协作

因此 Picoo Camera 不需要重新实现一套会议系统，也不需要替代飞书。

新的需求来自会议结束后的内容使用。

普通飞书会议录像通常仍然是传统会议布局：

- 人物以矩形摄像头窗口存在
- 屏幕共享和人物相互割裂
- 远程嘉宾的视频经过网络和会议软件压缩
- 很难得到 Riverside 那种自然的双人/多人对谈画面

希望 Picoo 能够在**完全不改变飞书会议习惯**的情况下，为会议额外记录高质量素材，并在会后自动生成更适合发布的视频。

## 3. 核心场景

例如一次三人会议：

- 主持人在飞书开会
- 嘉宾 A、嘉宾 B 正常加入飞书
- 主持人正常共享 PPT、网页或者产品 Demo
- 所有人正常交流，不需要进入另外一套会议系统

与此同时，Picoo 在各自设备上独立记录本地摄像头。

```text
                     飞书会议

              Host ───────── Guest A
                \             /
                 \           /
                  ── Guest B ──

           实时会议 / 音频 / 屏幕共享
```

Picoo 在旁边完成：

```text
Host Camera    → host.mp4
Guest A Camera → guest-a.mp4
Guest B Camera → guest-b.mp4
Screen         → screen.mp4
```

会议结束以后，再统一进行后处理。

## 4. 最终希望得到的效果

普通会议录像：

```text
┌─────────────────────────────┐
│          Screen             │
├──────────┬──────────┬───────┤
│  Host    │ Guest A  │Guest B│
└──────────┴──────────┴───────┘
```

Picoo 最终成片：

```text
┌──────────────────────────────────┐
│                                  │
│          PPT / Demo              │
│                                  │
│                                  │
│   👤 Host              Guest 👤  │
│                                  │
└──────────────────────────────────┘
```

人物经过抠图以后成为独立图层，可以自然地放在共享内容前面。

整体体验接近 Riverside 的访谈、演示和 Podcast 视频风格。

## 5. Picoo 的定位

Picoo 不负责 Meeting。

Picoo 负责 Recording。

```text
飞书
│
└── 实时会议
    ├── 视频
    ├── 音频
    ├── Screen Share
    └── 协作

Picoo
│
└── 高质量录制
    ├── 独立人物轨
    ├── 屏幕轨
    └── 会后视频生成
```

两套系统可以同时工作，但互不依赖。

产品关系可以总结为：

```text
飞书
=
让团队把会议开好

Picoo Camera
=
让摄像头效果更好

Picoo Recording
=
让会议结束后的内容更好
```

最终目标是：

> 团队继续像以前一样在飞书开会，但会议结束以后，可以直接得到接近 Riverside 风格的高质量对谈视频。

## 6. 视频来源

Picoo 不要求所有成员必须使用手机。

每个参与者只需要能够提供自己的**本地独立摄像头轨道**。

可以来自：

```text
Picoo Mobile Camera
USB Webcam
Mac / Windows 内置摄像头
相机 + Capture Card
```

例如：

```text
Alice
Picoo Mobile
→ 4K Local Recording

Bob
USB Webcam
→ 1080p Local Recording

Carol
Laptop Camera
→ 1080p Local Recording
```

这些素材最后统一进入同一个 Recording Session。

手机只是更高质量的视频源，并不是整个系统的必要条件。

## 7. 整体架构意图

```text
                 ┌──────────────┐
                 │    飞书会议    │
                 │              │
                 │ 实时交流/共享  │
                 └──────┬───────┘
                        │
                 正常进行会议


同时：

Host Camera ─────────────┐
                         │
Guest A Camera ──────────┤
                         │
Guest B Camera ──────────┼──→ Picoo Recording Session
                         │
Screen Capture ──────────┘
                              │
                              ▼
                          会议结束
                              │
                              ▼
                         自动时间同步
                              │
                              ▼
                         人物视频抠图
                              │
                              ▼
                         自动布局合成
                              │
                              ▼
                          Final Video
```

这是产品意图，不是当前仓库的实现架构。后续若进入 Design Specs，需要单独判断 Recording Session、素材轨、同步、抠图和合成各自落在哪条架构边界上。

## 8. 后处理

会议结束以后，Picoo 将不同的视频轨道自动整理。

核心流程保持简单：

```text
独立视频轨
+
屏幕共享轨
        ↓
      同步
        ↓
    人物抠图
        ↓
    自动排版
        ↓
     视频输出
```

由于允许离线处理，因此可以使用质量更高的人像 Matting 模型，而不需要为了实时性牺牲效果。

这也是 Picoo 有机会做到接近 Riverside 成片效果的重要原因。

## 9. Picoo Camera 与 Recording 的关系

Picoo Camera 原有功能保持不变：

```text
手机
 ↓
Picoo Desktop
 ↓
Virtual Camera
 ↓
飞书
```

新增：

```text
Picoo Recording
```

可以同时把同一个摄像头保存成本地高质量素材。

例如：

```text
Camera
   │
   ├──→ Picoo Virtual Camera → 飞书
   │
   └──→ Local Recording      → 后期素材
```

如果用户没有使用 Picoo 手机端：

```text
Webcam
   │
   ├──→ 飞书
   │
   └──→ Picoo Recording
```

同样成立。

## 10. 产品边界

本方案明确不包含：

- 自己的多人会议系统
- WebRTC Conference
- 聊天
- 会议邀请
- 日程系统
- 实时多人导播
- 替代飞书

重点只做好：

**获取高质量独立素材，并自动生成漂亮的视频。**

## 11. 与当前产品基线的关系

当前 [产品基线 V1.0](../product/picoo-camera-prd-v1.0-2026-08-27.md) 与 [BUC-001](../design-specs/use-cases/business/buc-001-phone-as-wireless-meeting-camera.md) 的职责是：把手机变成电脑会议软件可选用的高质量无线摄像头。

其中已经明确不在 V1.0 范围的能力包括：

- 本地录像
- 多手机同时输入
- 4K
- AI 美颜、背景替换、虚化
- 账号、组织与云同步

本文描述的 Picoo Recording 会重新碰到这些能力，但方向不同：

| V1.0 Picoo Camera | 本文 Picoo Recording |
| --- | --- |
| 实时把一路手机画面送进飞书 | 为会后成片保存多路独立素材 |
| 视频默认不保存 | 本地高质量录像是核心输入 |
| 单手机、单 Receiver、单虚拟摄像头 | 主持人和嘉宾各自贡献独立人物轨 |
| 实时性优先，可丢旧帧 | 离线处理，可追求更高抠图与合成质量 |
| 手机是必要视频源 | 手机是更高质量视频源，不是必要条件 |

因此本文是**新的产品方向研究**，不是 V1.0 范围扩写。在后续需求讨论确认之前：

- 不把 Recording Session、本地录像、多机位、抠图或自动成片写入当前 Use Case / Architecture / Requirement。
- 不把本文当作 V1.0 验收范围。
- Picoo Camera 现有虚拟摄像头主线保持不变。

## 12. 后续待讨论的问题

这些问题先记下，不在本文内作架构或实现结论：

1. Recording Session 如何把不同设备上的独立素材轨收成一次会议，而不引入 Picoo 自己的会议系统。
2. 时间同步以什么为基准：本地时钟、会议开始标记、音频对齐，还是其它信号。
3. 屏幕轨从哪里来：主持人电脑采集、飞书共享内容，还是两者都可能。
4. 音频轨如何处理：继续只用电脑麦克风、各端本地录音，还是会后从飞书录像提取。
5. 嘉宾没有 Picoo 手机、只有普通 webcam 时，桌面端如何同时服务飞书实时画面和本地录像。
6. 人像抠图、自动排版和成片是本地离线能力，还是未来才考虑的独立处理面。
7. Picoo Recording 是 Camera 的附加能力，还是需要单独的产品表面与术语。
8. 若这条线成立，哪些能力需要进入新的产品基线，哪些继续留在研究层。

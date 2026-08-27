# Use Cases

本目录维护两层长期 Use Case：Business Use Case 与 Product Use Case。

## Business Use Case

`business/` 描述用户希望完成的完整业务成果，使用 `BUC-###` 作为稳定 ID。

当前入口：

- [BUC-001：使用手机作为会议与录制的高质量无线摄像头](business/buc-001-phone-as-wireless-meeting-camera.md)

## Product Use Case

`product/` 描述用户通过 Pico Camera 完成目标时可感知的产品行为，使用 `PUC-###` 作为稳定 ID。

完整索引见 [Product Use Cases](product/README.md)。

## 追溯关系

```text
Business Use Case
  -> supported by Product Use Case
  -> constrained by Architecture
  -> decomposed into Requirement
  -> implemented and verified by code / test / config
```

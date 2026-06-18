---
name: jiuflow_device_preference
description: JiuFlow app must always deploy to physical iPhone, never simulator
type: feedback
---

flutter run は常に物理iPhone (00008140-0005453411E0801C) にデプロイする。シミュレーターは使わない。

**Why:** ユーザーが実機で確認したいため。ワイヤレスデバッグはタイムアウトしやすいので `--device-timeout 120` を付ける。

**How to apply:** `flutter run -d 00008140-0005453411E0801C --device-timeout 120`
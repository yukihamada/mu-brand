---
name: feedback_radio_systemd
description: Soluna radio processes are managed by systemd — never start manually with nohup
type: feedback
---

Soluna radio (soluna-radio-cpp) on relay.solun.art is managed by systemd services (soluna-radio-{bjj,jazz,chill,dance,lofi,soluna,yuki}.service). Never start radio processes manually with `nohup` or `&` — systemd auto-restarts them, causing duplicate processes that send double packets and corrupt audio (interleaved ADPCM states = noise).

**Why:** Spent hours debugging "distorted audio" that was caused by 2x radio processes per channel (1000pps instead of 500pps). Each encoder has independent ADPCM state, so interleaved packets produce garbage.

**How to apply:**
- Restart: `sudo systemctl restart soluna-radio-jazz`
- Restart all: `for ch in bjj jazz chill dance lofi soluna yuki; do sudo systemctl restart soluna-radio-$ch; done`
- Stop: `sudo systemctl stop soluna-radio-jazz`
- Check: `ps aux | grep radio-cpp | grep -v grep | awk '{print $NF}' | sort | uniq -c` (each channel should show exactly 1)
- Verify rate: Python UDP test should show ~600 pps (500 audio + ~100 FEC), NOT ~1200
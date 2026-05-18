---
name: koe_device
description: Koe Device project — koe.live, ESP32-S3 hardware, Soluna P2P audio mesh, 5 form factors
type: project
---

## Koe Device (koe.live)

**Vision:** 群衆を楽器にするデバイス。1台は記憶、100台はオーケストラ。

**Domain:** koe.live (Cloudflare DNS → GitHub Pages)
**Repo:** https://github.com/yukihamada/koe-device
**Site:** https://koe.live (EN/JA/Soluna Edition/Dashboard/Docs)

### 5 Form Factors
- Pick (guitar pick pendant, 30x30x8mm)
- Ear Cuff (titanium, clips on ear)
- Coin (26mm disc, 500 yen coin size)
- Band (wristband + speaker grille)
- Lantern STAGE (Pi CM5, 360° cylindrical, events)

### Architecture
- **CROWD devices:** ESP32-S3 + INMP441 mic + MAX98357A amp + GPS + LTE-M
- **STAGE devices:** Raspberry Pi CM5 + HiFiBerry DAC + TPA3255 + coaxial driver + GPS + 4G
- **Soluna mode:** UDP multicast 239.42.42.1:4242, GPS atomic clock sync, speed-of-sound correction
- **Koe mode:** AI voice assistant via chatweb.ai API

### Key Files
- firmware/demo/ — sync demo (2 ESP32 boards)
- manufacturing/ — JLCPCB BOM/CPL/PCB spec
- regulatory/ — 技適 self-declaration template (ESP32-S3-MINI-1: 201-220017)
- BUY_NOW.md — Amazon.co.jp parts list (~¥5,700)

### Domain Ecosystem
- koe.live → Koe device (hardware product)
- koe.elio.love → Koe software (macOS/Windows voice input)
- solun.art → Soluna events (ZAMNA HAWAII etc.)
- elio.love → Elio AI app
# 1本の動画を8言語に — AIリップシンク翻訳の作り方（実運用版）

JiuFlow の創設者・村田良蔵のメッセージ動画(日本語1本)を、**音声もリップシンク(口の動き)も8言語**に変換して各言語ページに埋め込みました。しかも元動画に焼き込まれていた日本語字幕を、各言語の字幕に差し替えています。

全部 Claude Code から API を叩いて自動化。手順をそのまま公開します。コピーして使ってOK。

---

## まず観てください（各言語版）

同じ村田の動画が、各言語で「その言語の音声 + その言語の字幕 + 口の動きも合ってる」状態になっています。右上の🌐で切替も可能。

| 言語 | リンク |
|---|---|
| 🇯🇵 日本語（元） | https://jiuflow.com/about |
| 🇺🇸 English | https://jiuflow.com/en/about |
| 🇪🇸 Español | https://jiuflow.com/es/about |
| 🇧🇷 Português | https://jiuflow.com/pt/about |
| 🇰🇷 한국어 | https://jiuflow.com/ko/about |
| 🇫🇷 Français | https://jiuflow.com/fr/about |
| 🇮🇩 Indonesia | https://jiuflow.com/id/about |
| 🇩🇪 Deutsch | https://jiuflow.com/de/about |

---

## 仕組み（5ステップ）

```
YouTube(JP動画)
   │ ① yt-dlp で 1080p DL
   ▼
Cloudflare Stream（元動画ホスト）
   │ ② HeyGen video_translate API
   ▼
各言語の dub（音声翻訳 + リップシンク）+ caption_url（その言語の字幕SRT）
   │ ③ ffmpeg で焼き込みJA字幕を黒帯マスク + 各言語SRTを焼き込み
   ▼
Cloudflare Stream（言語別に再アップ）
   │ ④ サイトに言語別 <video> で埋め込み
   ▼
/{lang}/about で再生
```

### キモになった3つの発見

1. **HeyGenは「音声」だけ翻訳する。字幕は別問題。**
   元動画にピクセルとして焼き込まれた日本語字幕は HeyGen では消えません。→ ffmpeg で字幕帯を背景色の箱(drawbox)で隠し、HeyGen が返す `caption_url`(各言語の字幕データ)を改めて焼き込む。

2. **HeyGen の翻訳出力は360pまで。**
   HD download エンドポイントが無いので、360p出力を lanczos で1080pにアップスケール。フレームサイズはHD、ディテールは360p相当。

3. **動画のダウンロードを先に有効化しないと失敗する。**
   Cloudflare Stream は MP4 download を明示的に有効化(`POST /stream/{uid}/downloads`)しないと、HeyGen が「動画を取得できない」で落ちる。バッチ翻訳で最初の数十本が無音で失敗した原因がこれ。

---

## Claude Code でどうやるか（再現手順）

Claude Code に「この動画を多言語リップシンクして」と言うと、実際にこういう流れで動きます。コマンドはそのままコピペで動きます。

### 0. 準備（APIキー）
```bash
# HeyGen（音声翻訳+リップシンク） … 動画尺ぶんクレジット消費（≈$0.5/分）
HEYGEN_KEY="sk_..."
# Cloudflare Stream（動画ホスト） … グローバルキーを使う
source /Users/yuki/.env   # CLOUDFLARE_EMAIL + CLOUDFLARE_API_KEY
CF_ACCOUNT="..."
```

### 1. YouTube から1080pでDL
```bash
yt-dlp --remote-components ejs:github \
  -f "bestvideo[height<=1080]+bestaudio" --merge-output-format mp4 \
  -o "src.mp4" "https://youtube.com/watch?v=XXXX"
```
※ `--remote-components ejs:github` が無いと JS challenge で360pに落ちる(ハマりどころ)。

### 2. Cloudflare Stream にアップ + ダウンロード有効化
```bash
# アップ
curl -X POST "https://api.cloudflare.com/client/v4/accounts/$CF_ACCOUNT/stream" \
  -H "X-Auth-Email: $CLOUDFLARE_EMAIL" -H "X-Auth-Key: $CLOUDFLARE_API_KEY" \
  -F file=@src.mp4
# 返ってきた uid で MP4 download を有効化（これ必須）
curl -X POST ".../stream/$UID/downloads" -H "X-Auth-Email: …" -H "X-Auth-Key: …"
```

### 3. HeyGen で各言語に翻訳（音声+リップシンク）
```bash
curl -X POST "https://api.heygen.com/v2/video_translate" \
  -H "x-api-key: $HEYGEN_KEY" -H "Content-Type: application/json" \
  -d '{"video_url":"<CF mp4 url>","output_language":"Spanish","title":"..."}'
# → video_translate_id。ポーリングで status=success を待つと
#    data.url(翻訳済みmp4) と data.caption_url(その言語の字幕) が返る
```

### 4. JA字幕をマスク + 各言語字幕を焼き込み（ffmpeg）
```bash
# 焼き込み日本語字幕の帯を背景色の箱で隠し、SRTを overlay
# （このMacのffmpegはlibass無しだったので、字幕はPILで透明オーバーレイ動画を
#   生成して合成。libassがあれば subtitles フィルタ一発でOK）
ffmpeg -i dub.mp4 -i sub_overlay.mov \
  -filter_complex "[0]scale=1920:1080,drawbox=x=300:y=850:w=1320:h=215:color=0xbfbcb7:t=fill[m];[m][1]overlay" \
  out.mp4
```

### 5. 再アップ + サイトに言語別埋め込み
```html
<video controls src="{% if lang=='es' %}…/es_uid/…{% else if lang=='pt' %}…{% endif %}">
```

これを動画の本数 × 言語数だけ回すバッチを Claude Code が書いて、ポーリング・再投入・DB反映・git push まで自律で回しました。

---

## コスト感

- HeyGen: **動画の尺ぶん**消費(≈$0.5/分)。3分の動画を7言語 = 約21分 ≈ $10。
- Cloudflare Stream: 保存 + 配信(従量、わずか)。
- yt-dlp / ffmpeg: 無料。

「1本撮って8言語に配る」が動画1本あたり数ドルで回る時代。翻訳・吹き替えスタジオに頼むと1言語数万円〜の世界が、AIで桁が変わりました。

---

## FB 投稿（そのまま使える下書き）

> 🥋 JiuFlow の創設者メッセージ、**8言語**に対応しました。
>
> 日本語で撮った1本の動画を、AIで「音声・口の動き・字幕」ぜんぶ各言語に。スペイン語の人にはスペイン語で、ポルトガル語の人にはポルトガル語で、村田良蔵が語りかけます。
>
> 🇯🇵🇺🇸🇪🇸🇧🇷🇰🇷🇫🇷🇮🇩🇩🇪 — 👉 https://jiuflow.com/about（右上🌐で言語切替）
>
> やり方は全部公開してます。動画1本を多言語化するのに、もう翻訳スタジオは要りません。HeyGen + Cloudflare + ffmpeg + Claude Code で、1本あたり数ドル。
> 詳細 → https://wearmu.com/vault/jiuflow-lipsync
>
> #BJJ #柔術 #JiuFlow #AI #lipsync #多言語

---

*この記事は wearmu.com の VAULT(Tシャツ所有者向けの「裏側」公開）に置いています。MU も JiuFlow も、作り方を隠さない方針です。*

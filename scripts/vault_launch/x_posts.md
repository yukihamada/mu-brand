# X (Twitter) 投稿テンプレ集 — vault launch

「みんなが書きたくなる」=
①話題性が高く ②自分も賢く見え ③シェアの摩擦が低い
の3条件を満たすことを意識した素材。

---

## 🚀 A. 創業者から (発射用、launch day)

### A1. メインアナウンス (短く、画像1枚)
```
MUのTシャツを買った人だけが入れる場所を作りました。

中身は3つ。
1. 全コードベース解説 (Rust+Gemini+Printful)
2. 実運用してるGemini 3プロンプト10本
3. リアルタイム原価帳簿

服が鍵になる、というのを文字通りやってみた。

https://wearmu.com/vault
```
→ 画像: vault の dashboard 画面のスクリーンショット (LIVE 表示が映ってる状態)

### A2. 哲学投稿 (リプライまたは別tweet)
```
普通のEC: 在庫・原価・マージンは秘密
MU: 利益の50%を弟子屈町に寄付するので、実態より高く見せる動機がない

→ 寄付額をリアルタイムで証明する方が、お客様 (=寄付の受益者にとっても価値ある人) との信頼関係が強くなる

これがvault公開の動機です
```

### A3. テクニカル深堀り (引きを作る)
```
vaultで公開してるGemini 3プロンプトの1つ:

「ad keywordをdesignに書き込んじゃう問題」の解決法

→ promptにkeyword混ぜる時、prefix `[Ad keyword: ...]` で囲む + Gemini側にstripさせる

これ我々ハマって何枚も無駄にしました

詳細: https://wearmu.com/vault/prompt-cookbook
```

### A4. 数字での透明性
```
今日のMU
売上 ¥XX,XXX
寄付累計 ¥XX,XXX
GPU稼働 78%
agent log: mugen_drop #197 OK / printful_sync 12orders pending

これ全部Tシャツ所有者にはリアルタイム見えてます。
ライブダッシュボード: https://wearmu.com/vault (要ログイン)
```

---

## 🎯 B. お客様シェア用 (引用RT / 自然な拡散誘発)

### B1. 開けた直後の素直なリアクション系
```
MUのTシャツ買ったらvault開いた

中、こんな感じ ↓

(スクショ)

たかが¥5,000のTシャツに、運用してるプロンプトと原価帳簿と本日の売上が付いてくる。狂ってる(褒め言葉)
```

### B2. 「実は」を語る系
```
MU、Tシャツ持ってる人だけ入れる場所があるんだけど

そこで「ADCC使ったらGoogle Adsで商標fail」とか「→記号もSYMBOLS policyでreject」とか実運用の失敗が全部書いてある

普通こういうの隠すよね。これは買って読むタイプの本
```

### B3. プロンプト引用シェア系
```
MUのvaultで貰った1個:

「Yes-man的な賞賛をAIは出しがち。"No empty praise"と明示すると質が劇的に上がる」

founderへの日次standup用prompt の話

天気でTシャツ売ってる会社だと思ったらAIラボでもあった
```

### B4. 「これでTシャツの意味が変わる」系
```
MUのTシャツ = 物理的な布 + vault accessキー

布だけ見ると¥5,000は高い
布 + AIラボ + 原価帳簿 + 寄付の証明 で見ると安い

EC のあり方、こうなっていく予感
```

### B5. 引きの強い1行系 (RT促進)
```
"利益の50%を弟子屈町に寄付するので、実態より高く見せる動機がない"

— MU vault より
```

---

## 🪝 C. 引きを作るための「これ知ってる?」系 (vault導入)

### C1. ヒント投稿 (vault誘導)
```
日本のスモールECで、購入者だけが見られるdashboard持ってるとこ、どれくらいある?

MU はこうしてる ↓
- 全コード解説
- 全プロンプト
- リアルタイム売上/寄付
- AI agent journal

https://wearmu.com/vault
```

### C2. founder の素直な悩み系
```
1着¥5,000のTシャツに付加価値どう乗せるか問題

→ 「物理製品」だけだとShein/Temuに負ける
→ 「ストーリー」だけだとnoteで足りる
→ 「実装も哲学も全部open + 購入者にだけ深層access」が我々の解

vaultやってみた感想あれば教えてください
```

---

## 🎨 D. 投稿時の運用メモ (founder用)

- **画像必須**: vault index ページのスクリーンショット (LIVE card が見えるやつ) を1枚は付ける
- **タイミング**: 平日 朝9-10時 or 夜21-22時 JST が一番伸びる (Japan tech audience)
- **ハッシュタグ**: 多くても2つまで `#wearmu` `#オープンソース`
- **リプライへの返答**: 「これコード見たい!」系は github.com/yukihamada/mu-brand 即返信
- **連投構成 (推奨)**: A1 → 2時間後 A2 → 1日後 A3 → 3日後 A4

## 🚫 やってはいけないこと

- 「革命的」「画期的」のような hype 表現 (お客様メモでも避けてる)
- 個別お客様の email/名前を出す
- 数字を盛る (現状の正確な数字が最強のコンテンツ)
- 「他のECは...」と他社disり

---

## E. 引用元 / 関連リンク (1ツイート内に貼る)

- vault: https://wearmu.com/vault
- 商品: https://wearmu.com/products/ads_jujitsu/1034 など
- about: https://wearmu.com/about
- 寄付方針 §27: https://wearmu.com/constitution#27
- GitHub: https://github.com/yukihamada/mu-brand

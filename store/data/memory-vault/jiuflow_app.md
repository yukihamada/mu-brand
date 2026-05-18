---
name: jiuflow_app
description: JiuFlow iOS + SSR architecture, API endpoints, tournament/game plan features, i18n setup
type: project
---

# JiuFlow App (iOS + SSR)

## アーキテクチャ (2026-03-22時点)

### SSR (Rust / Fly.io)
- **URL**: https://jiuflow-ssr.fly.dev
- **Path**: `bjj/jiuflow-ssr/`
- **Stack**: Axum 0.7 + Askama + SQLite (WAL) + Fly.io nrt
- **Deploy**: `cd bjj/jiuflow-ssr && fly deploy --remote-only -a jiuflow-ssr`

### iOS App (SwiftUI)
- **Path**: `bjj/jiuflow-swift/`
- **Min iOS**: 17.0
- **Bundle**: com.jiuflow.app
- **Build**: `xcodebuild -project JiuFlow.xcodeproj -scheme JiuFlow`
- **Project生成**: `xcodegen generate` (project.yml)
- **Design**: forced dark mode, DesignSystem.swift (jfRed=#DC2626, glass-card)

## API Endpoints (JSON)

| Method | Endpoint | Returns |
|--------|----------|---------|
| GET | `/api/v1/tournaments` | 全大会リスト (DB + enrichment 140+件) |
| GET | `/api/v1/tournaments/:year/:slug` | 大会詳細 + 結果 + 階級 |
| GET | `/api/v1/game-plans` | テンプレート17種 (JSON plan data含む) |
| GET | `/api/v1/videos` | 動画一覧 |
| GET | `/api/v1/athletes` | 選手一覧 |
| GET | `/api/v1/dojos` | 道場一覧 |
| GET | `/api/v1/news` | ニュース |
| GET | `/api/v1/forum/threads` | フォーラム |
| GET | `/api/v1/instructors` | インストラクター |
| GET | `/api/v1/technique-map` | テクニックツリー |
| GET | `/api/v1/technique-flow` | フロー図データ |

## 大会機能
- **データソース**: SQLite DB + tournament_enrichment.rs (140+静的) + tournament_results.rs (1000+結果)
- **結果データ**: IBJJF, ADCC等の金銀銅メダリスト、ディビジョン別・年別
- **iOS**: TournamentDetailNativeView (アプリ内完結、Webに飛ばない)
- **フィルター**: 協会別 (IBJJF/ASJJF/SJJJF/JBJJF/AJP/ADCC/JJFJ)

## ゲームプラン機能
- **テンプレート**: 9システム + 8プロ選手モデル = 17種
- **AI生成**: chatweb.ai SSE streaming → JSON parse
- **iOS**: 全てアプリ内ネイティブ表示 (Webリンクなし)

## 多言語 (i18n)
- **LanguageManager**: `@AppStorage("preferred_language")` ja/en/pt
- **使い方**: `lang.t("日本語", en: "English", pt: "Portuguese")`
- **API側**: `name_ja`/`name_en`, `description_ja`/`description` のデュアルフィールド
- **対応済み画面**: タブバー, 大会一覧/詳細, ゲームプラン, GamePlanDetail

## 方針
- **アプリからWebに飛ばさない** — 大会・ゲームプラン・ニュースは全てアプリ内完結
- **例外OK**: プライバシーポリシー, 利用規約, 外部登録URL, ブログ
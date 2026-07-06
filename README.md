# AlcAppTauri

Android WebView キオスクアプリ `ippoan/AlcoholChecker` を **Windows + [Tauri](https://tauri.app/)** で再実装するプロジェクト。詳細な設計・判断根拠は [`plan/00-tauri-windows-migration.md`](plan/00-tauri-windows-migration.md) を、進捗は tracking issue #1 を参照。

## 構成 (Phase 1)

案A (リモート表示): Tauri の WebView2 window が `https://alc.ippoan.org` を直接読み込む。認証 cookie (`.ippoan.org`) / Google OAuth はそのまま成立する。`dist/index.html` は起動時のローダーシェルで、表示先が到達可能になってからナビゲートし、オフライン時は自動リトライする (無人キオスク耐性)。

## 開発 / ビルド

前提: Rust (stable) / Node 20+ / Windows は WebView2 Runtime (Windows 11 は同梱)。

```sh
npm install
npm run dev     # 開発起動
npm run build   # NSIS インストーラをビルド (windows-latest)
```

表示先を staging に向ける:

```sh
ALC_APP_URL=https://alc-staging.ippoan.org npm run dev
```

| env | 既定 | 用途 |
|---|---|---|
| `ALC_APP_URL` | `https://alc.ippoan.org` | WebView が読み込む alc-app の URL |
| `ALC_RETRY_INTERVAL_MS` | `5000` | ローダーの再試行間隔 (ms) |

## CI

`.github/workflows/ci.yml` が `windows-latest` で fmt / clippy / test / `tauri build` (NSIS) を PR ごとに検証し、インストーラを artifact に上げる。

# AlcAppTauri

`ippoan/AlcoholChecker` (Android WebView キオスクアプリ) の Windows 版。
[Tauri](https://tauri.app/) でネイティブラッパーを実装し、既存の Web フロントエンド
(`ippoan/alc-app` の `web/`, Nuxt 4 PWA) をそのまま利用する。

- NFC: [`ippoan/rust-nfc-bridge`](https://github.com/ippoan/rust-nfc-bridge) (PC/SC) を統合
- FC-1200 (アルコールセンサー): [`ippoan/fc1200-wasm`](https://github.com/ippoan/fc1200-wasm) のプロトコル実装をネイティブ側で再利用 (予定)
- バックエンド: `ippoan/rust-alc-api` / `ippoan/auth-worker` (変更なし)

現在は計画段階。詳細は [`plan/00-tauri-windows-migration.md`](./plan/00-tauri-windows-migration.md) を参照。

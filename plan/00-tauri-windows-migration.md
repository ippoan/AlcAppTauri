# AlcoholChecker Windows (Tauri) 移植計画

対象: `ippoan/AlcAppTauri` (新規・空リポジトリ)
元ネタ: `ippoan/AlcoholChecker` (Android WebView キオスクアプリ)
ターゲット OS: **Windows**
NFC: **PC/SC** (`pcsc` crate、`ippoan/rust-nfc-bridge` と同方式)

## 0. 前提として確認した既存資産 (このリポジトリはまだ空、以下は調査結果)

Android 版 (`AlcoholChecker`) は「WebView + 複数のローカル WebSocket ブリッジ」という構成で、
Web 側 (`alc-app/web`, Nuxt 4 PWA) は **すでにプラットフォーム非依存に書かれている**。
これは Tauri 移植にとって非常に有利な事実:

| 機能 | Web 側の実装 | Android 側 (WebView) | デスクトップ Chrome (今も動く) |
|---|---|---|---|
| NFC 読取 | `useNfcWebSocket.ts` → `ws://127.0.0.1:9876` 固定 | `NfcBridgeServer` (WebSocket) | **`ippoan/rust-nfc-bridge` が既に提供** (Windows, PC/SC, MSI 配布済み) |
| FC-1200 (アルコールセンサー) | `useFc1200Serial.ts`: **WebSerial 優先 → 無ければ `ws://127.0.0.1:9878` にフォールバック** | `Fc1200BridgeServer` (WebSocket) + `Fc1200Protocol.kt` (Kotlin 再実装) | WebSerial 経由で `fc1200-wasm` (WASM) を直接使用 (ブリッジ無し) |
| BLE Gateway (体温計/血圧計) | `useBleGateway.ts`: WebSerial 優先 → `ws://127.0.0.1:9877` フォールバック | `BleBridgeServer` (WebSocket) | WebSerial (未検証) |
| 画面共有 (遠隔点呼) | `useScreenShare.ts`: `getDisplayMedia()` 直接 | `ScreenCaptureBridgeServer` (Android は WebView が非対応なため代替実装) | ブラウザ標準 API でそのまま動作 |
| カメラ顔認証 | `getUserMedia()` 直接 (`useFaceDetection.ts` 等) | Android WebView は対応 | 対応 |
| デバイス登録・認証 (`X-Tenant-ID` / device JWT) | `useDeviceToken.ts` / `DeviceRegistration.vue` 等、**完全に localStorage + REST** | 同じ Web ページをそのまま表示 | 同じ Web ページをそのまま表示 |
| 遠隔点呼 WebRTC | `useWebRtc.ts` (標準 WebRTC API) | 同上 | 同上 |

つまり **NFC は既に Windows 対応済み** (`rust-nfc-bridge` を使うだけ)。
**未着手なのは FC-1200 の Windows ネイティブブリッジ (`ws://127.0.0.1:9878`) だけ**
(Android 版は Kotlin で protocol を再実装しており、Web/WASM 版と合わせて実装が2重化している。
Windows 版では `fc1200-wasm` の Rust ソースを **wasm ではなくネイティブ crate として再利用**し、
3重目の再実装を避ける)。

## 1. 最重要の未検証事項 (Phase 0 で最初に潰す)

Tauri は OS 標準の WebView (Windows は **WebView2** / Chromium ベース) を使う。
WebView2 が以下の Web API をどこまでサポートするかで、後続フェーズの設計が変わる:

1. **Web Serial API** (`navigator.serial`) — 対応していれば FC-1200 / BLE Gateway は
   ブリッジ無しでそのまま動く可能性がある (デスクトップ Chrome と同じ経路)。
   未対応 or 権限 UI が出せない場合は WebSocket ブリッジ必須。
2. **`getDisplayMedia()` (画面共有)** — 遠隔点呼の管理者側で使用。
3. **`getUserMedia()` (カメラ)** — 顔認証で必須。WebView2 は対応実績あるはずだが、
   権限プロンプト (Windows のカメラ権限含む) の動作を実機確認する。
4. **WebRTC 全般** — 遠隔点呼の P2P 通話。

→ **Phase 0 の成果物**: 最小 Tauri アプリ + `alc-app/web` を読み込ませ、上記 4 点を
実機 (Windows) で確認した結果表。ここが「WebSerial 使えない」なら FC-1200/BLE は
必ずネイティブブリッジ行き (Phase 3) になるので、最初に確定させる。

## 2. 全体アーキテクチャ (案)

```
┌─────────────────────────────────────────────┐
│ Tauri アプリ (Windows, 1 プロセス)             │
│  ┌───────────────────────────────────────┐  │
│  │ WebView2                              │  │
│  │  = alc-app/web (Nuxt) をそのまま表示    │  │
│  │    (リモート URL 読込 or 静的バンドル)   │  │
│  └───────────────────────────────────────┘  │
│  Rust backend (src-tauri/)                   │
│  ├─ nfc bridge     (rust-nfc-bridge 統合)     │──PC/SC──▶ [NFCリーダー]
│  ├─ fc1200 bridge  (fc1200-wasm ネイティブ化)  │──COMポート──▶ [FC-1200]
│  ├─ (要 Phase0) ble bridge                    │──COMポート──▶ [BLE Gateway]
│  ├─ autostart / window kiosk 設定             │
│  └─ updater (tauri-plugin-updater)            │
└─────────────────────────────────────────────┘
        │ HTTPS
        ▼
  rust-alc-api (既存, 変更なし) / auth-worker (既存, 変更なし)
```

フロントエンドは **新規に書かない**。`alc-app/web` を Windows 経路として consume する
(下記 3.1 の方式のどちらか)。

## 3. オープンな論点 (実装着手前に決めたいこと)

### 3.1 フロントエンドの取り込み方式

| 案 | 内容 | 長所 | 短所 |
|---|---|---|---|
| A. リモート表示 | Tauri の window に `https://alc.ippoan.org` 等を直接読み込む | 実装最小、alc-app 側のデプロイがそのまま反映される | オフライン起動不可、Tauri アプリと Web 側のバージョンが独立してズレる |
| B. 静的バンドル | `nuxt generate` (or `nuxi build` の static) した成果物を Tauri にバンドルし `tauri://localhost` で配信 | オフライン起動可、Tauri のバージョン管理下に置ける | alc-app 側の変更のたびに **AlcAppTauri 側も再ビルド・再配布**が必要。API 呼び出し系 (server routes) を持つ現行 alc-app/web が `nuxi generate` と相性が良いか要確認 |
| C. ハイブリッド | 初回は静的シェルを埋め込み、実際のページは `NUXT_PUBLIC_API_BASE` 等と同様に staging/prod URL を `<iframe>` または直接ナビゲーションで読む | A/B の中間 | 複雑 |

**現時点のおすすめは A (リモート表示)**: キオスク端末は常時オンライン前提(アルコール
チェック結果を都度送信するため)であり、Android 版も同じくオンライン前提の WebView。
Tauri アプリ自体は「ブラウザ chrome を持たないブランド化されたウィンドウ + ネイティブ
ブリッジ (NFC/FC-1200)」の役割に絞り、Web 資産は alc-app の既存デプロイパイプライン
(`test.yml` → staging / `v*` タグ → prod) にそのまま乗せる。これなら Web 側の変更に
Tauri 側の再ビルドが不要。

### 3.2 NFC ブリッジの統合方式

| 案 | 内容 |
|---|---|
| A. sidecar 同梱 | `rust-nfc-bridge` のリリースバイナリ (`nfc-bridge.exe`) を Tauri の external binary (sidecar) として同梱し、アプリ起動時に子プロセスとして起動。**既存資産をそのまま流用、変更ゼロ** |
| B. ネイティブ統合 | `rust-nfc-bridge` の `pcsc` 読み取りロジックを crate 化して `src-tauri` に直接組み込み、1 プロセスに統合 |

**おすすめは A**: `rust-nfc-bridge` は既に MSI 配布・Windows サービス化まで完成しており、
無停止で動く既存資産。Tauri 側は「起動時に既に `NfcBridge` サービスが動いていなければ
sidecar として起動する」程度の関与に留め、二重実装を避ける。

### 3.3 FC-1200 ブリッジ (新規実装が必要)

`fc1200-wasm` (`parser.rs` / `state_machine.rs` / `session.rs` / `commands.rs` / `modes.rs`,
計 1600 行) は wasm-bindgen 向けだが、コアロジックは `wasm_bindgen` 依存を薄く分離できる
はず。方針:

1. `fc1200-wasm` 側でコアロジックを `#[cfg(feature = "wasm")]` で任意化し、素の Rust
   crate としても import 可能にする (要 `ippoan/fc1200-wasm` 側の改修、別 issue)。
2. `AlcAppTauri` 側に `fc1200-bridge` モジュールを新規実装:
   - `serialport` crate で COM ポート (9600bps / 8N1、README 記載の設定) を開く
   - `fc1200-wasm` のコアロジックで RS232C フレームを解釈
   - `ws://127.0.0.1:9878` で Android 版と同じ JSON プロトコルを喋る
     (`useFc1200Serial.ts` は無改修で動く)
3. Phase 0 で WebSerial が使えると判明した場合は、この工程を **スキップ**して
   `fc1200-wasm` を wasm のまま WebView2 内で直接使う経路に倒せる可能性がある
   (要 Phase 0 の結果次第で判断)。

**機密性の扱い**: `fc1200-wasm` は「プロトコル実装を秘匿するために WASM 化」という
経緯 (Tanita Confidential 資料に基づく実装のため)。ネイティブ化してもソース非公開の
private repo 依存に変わりはなく、コンパイル済みバイナリの解析難易度は wasm と大差ない
という理解で進めるが、懸念があれば要相談。

### 3.4 BLE Gateway ブリッジ

体温計/血圧計連携が Windows キオスクでも必要か要確認。必要なら FC-1200 と同じ設計
(sidecar or 統合、`ws://127.0.0.1:9877`) を踏襲。不要なら Phase 4 ごとスキップ可能。

### 3.5 デバイス認証・登録

**追加実装は基本的に不要**。`useDeviceToken.ts` / QR・URL 登録ページは purely
web + localStorage + REST なので、Tauri の WebView 内でそのまま動く見込み。
確認するのは localStorage の永続化 (Tauri の WebView2 データディレクトリが
アプリ再起動をまたいで保持されるか) のみ。

### 3.6 キオスクロックダウン

Android の Device Owner 相当は Windows では Tauri アプリの責務ではなく、
**Windows OS 側の「割り当てられたアクセス (kiosk mode)」**または単純に:

- `tauri.conf.json` で `decorations: false` (タイトルバー無し) / `fullscreen: true` /
  ウィンドウクローズ防止 (`prevent_close` + 確認ダイアログ or 完全無効化)
- 右クリックメニュー・devtools・キーボードショートカット (Alt+F4 等) の無効化
- Windows 起動時の自動起動 (`tauri-plugin-autostart` or レジストリ Run キー)
- (オプション) Windows Shell 自体をこのアプリに差し替える「Kiosk browser」構成
  (OS 側設定、リポジトリの scope 外)

を組み合わせる。QR スキャンでの Device Owner プロビジョニング相当は Windows には
無いので、初期セットアップは手動 (アプリインストール → デバイス登録ページで
ペアリング) に簡略化される。

### 3.7 自動アップデート

Android 版は PR → dev 端末 OTA push (FCM) / master → GitHub Release、という
2 チャネル運用 (`alcoholchecker-deploy` skill 参照)。Tauri では:

- `tauri-plugin-updater` + GitHub Releases (`latest.json` manifest) を使い、
  起動時 or 定期的に更新チェック → 自動ダウンロード・再起動
- dev / prod のチャネル分離が要るなら、Android 版と同様に
  `dev-tag-release.yml` (`ci-workflows`) で dev チャネル、`tag-release.yml` で
  prod チャネルを分け、`updater` の endpoint を channel ごとに出し分ける
- 段階配信 (Release Wave 的な「明示トリガーで配信」) が要るかどうかは
  運用ポリシー次第。まずは「PR merge で dev 端末に自動配布、prod は手動トリガー」
  という Android 版の運用を踏襲する案を軸に相談

### 3.8 遠隔点呼 (WebRTC)

Android は着信のためにバックグラウンド `RoomWatcher` + FCM full-screen intent が
必要だった (端末がロック/バックグラウンドになり得るため)。Windows キオスクは
「常時フォアグラウンドの画面」前提なので、**既存の `TenkoKiosk.vue` の
`remoteMode` prop によるページ内ポーリングだけで足り、ネイティブ常駐監視は
恐らく不要** (Phase 0 検証後に確定)。

## 4. フェーズ計画

| Phase | 内容 | 成果物 / 完了条件 |
|---|---|---|
| **0. 実機検証** | 最小 Tauri アプリで WebView2 の WebSerial / getDisplayMedia / getUserMedia / WebRTC 対応を確認 | 対応状況の一覧表 (3.3〜3.8 の設計判断がこれで確定する) |
| **1. 雛形構築** | `cargo tauri init`、`alc-app/web` (staging URL) を表示するだけの window。CI (`windows-latest` build) 疎通確認 | ビルド済み `.exe` が起動し alc-app のログイン/キオスク画面が出る |
| **2. NFC 統合** | `rust-nfc-bridge` sidecar 同梱・自動起動、実カードで `nfc_read` イベント疎通確認 | 実機で NFC タップ→Web側イベント受信 |
| **3. FC-1200 ネイティブブリッジ** | (Phase 0 結果次第) `fc1200-wasm` コアロジック流用の native bridge 実装、実センサーで疎通確認 | 実機で FC-1200 測定→Web側にデータ到達 |
| **4. BLE Gateway (要否確認後)** | 同上パターン | 要否確定後に判断 |
| **5. キオスク UX** | フルスクリーン・chrome無し・自動起動・終了防止 | 実機で「起動したらそのままキオスク画面、再起動しても自動復帰」 |
| **6. デバイス認証確認** | 既存 Web ページでのペアリング・localStorage 永続化確認 | 実機で再起動後もログイン/デバイス登録状態が保持される |
| **7. 自動アップデート** | `tauri-plugin-updater` 配線、dev/prod チャネル設計、CI release workflow | タグ push で `.msi`/`.exe` が Release に上がり、既存端末が自動更新される |
| **8. CI/CD** | `windows-latest` runner での build/test/release、`ci-workflows` 標準に揃える | PR ごとに build 検証、タグで自動リリース |
| **9. 実機ロールアウト** | 1台での実運用テスト → 展開 | 運用判断 (このリポジトリの scope 外) |

## 5. 主なリスク

- **WebView2 の Web API 対応状況が未確認** (Phase 0 で最優先に潰す。ここがボトルネック)
- `fc1200-wasm` のコア分離 (wasm 依存を外す) が想定より大掛かりになる可能性
  → 最悪 native 版のプロトコル実装を素直に再実装 (Kotlin 版が既にあるので参考にはなる
    が、また3重目の実装になり避けたい)
- BLE Gateway / 画面共有の要否が未確定 (要ヒアリング)
- 自動アップデートの配信チャネル運用 (dev/prod 分離の要否) が未確定

## 6. 次のアクション

1. (要判断) 3.1〜3.4, 3.7 の各論点について方針を確定
2. Phase 0 (実機検証) から着手 — Windows 実機が無い場合は代替の検証手段を相談

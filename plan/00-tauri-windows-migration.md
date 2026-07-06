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

### 3.4 BLE Gateway ブリッジ (M5Stack 経由のシリアル方式で確定)

体温計/血圧計 (BLE) との連携。**Windows ネイティブの BLE スタックは信頼性が
不安なため使わず、M5Stack (ESP32) を BLE ↔ USB シリアル変換ゲートウェイとして
間に挟む**方針で確定。Windows 側はネイティブ BLE を一切触らず、M5Stack を
**USB COM ポート (115200bps) 経由で読むだけ**になる。

- これは既存 `useBleGateway.ts` の前提と完全に一致: 同 composable は既に M5Stack 系
  デバイス (CH340 / CP210x / Espressif native USB / FTDI FT232R、115200bps) を対象に
  しており、WebSerial 優先 → `ws://127.0.0.1:9877` フォールバックの2経路を持つ。
- よって Windows 版のブリッジ設計は **FC-1200 (3.3) と全く同じシリアルブリッジ**:
  `serialport` crate で M5Stack の COM ポートを開き、`ws://127.0.0.1:9877` で
  Android 版と同じ JSON プロトコルを喋る (`useBleGateway.ts` は無改修で動く)。
- Windows ネイティブ BLE (WinRT `Windows.Devices.Bluetooth`) 実装は **不要** =
  リスクの高い部分を M5Stack ファーム側に隔離できるのが利点。
- M5Stack のファームウェア自体はこのリポジトリの scope 外 (別途管理)。本リポジトリは
  「M5Stack が吐くシリアル JSON をそのまま WebSocket に橋渡しする」ことに徹する。
- Phase 0 で WebView2 の WebSerial が使えると分かれば、このブリッジすら不要で
  WebSerial 直結に倒せる (FC-1200 と同じ判断)。

### 3.5 認証 (auth-worker) — Tauri 移植の設計を左右する最重要事項

alc-app の認証は **`ippoan/auth-worker` が全面的に担う** (rust-alc-api#434、rust は
dumb backend)。フローは以下 (`useAuth.ts` 実測):

```
WebView (alc.ippoan.org) ─ /login の Google ボタン
  → window.location = auth.ippoan.org/oauth/google/redirect?redirect_uri=<callback>
  → auth-worker が Google と code 交換 → JWT 発行
  → auth-worker が logi_auth_token cookie (Domain=.ippoan.org) を set
  → redirect_uri (=<origin>/auth/callback) に戻す
  → alc-app は cookie を読むだけ (cookie が唯一の真実)
```

キオスク端末側は別経路もある:
- **device JWT** (`useDeviceToken.ts`): auth-worker `/device/pair` (role=device-kiosk) で
  発行した device credential を localStorage 保持 → `/device/token` で 1h JWT を mint →
  `/api/proxy/*` に Bearer 送信
- **auth バイパス** (staging): `NUXT_PUBLIC_STAGING_TENANT_ID` で `X-Tenant-ID` 直送

**Tauri 移植への含意 (これが 3.1 の方式選択を決める)**:

- 認証はすべて **cross-subdomain cookie (`.ippoan.org`) + full-page OAuth redirect** に
  依存している。Android 版はリモート alc-app URL を WebView に読ませているので、これが
  そのまま成立していた。
- **案A (リモート表示) なら WebView2 のオリジンが `alc.ippoan.org` になり、cookie も
  OAuth redirect も Android WebView と完全に同じく透過的に動く** → 認証まわりの追加実装ゼロ。
- **案B (静的バンドル, `tauri://localhost`) は認証を壊す**:
  1. `logi_auth_token` は `Domain=.ippoan.org` の cookie。`tauri://localhost` オリジンには
     送られない (別サイト扱い)。
  2. `redirect_uri` が `tauri://localhost/auth/callback` になり、auth-worker の
     `ALLOWED_REDIRECT_ORIGINS` 許可リスト + Google 側 OAuth redirect 制約に引っかかる。
  3. カスタムスキームの cookie / OAuth ハンドリングを自前で補う必要が生じる。
  → 案B を採るなら **auth-worker 側の改修 (custom-scheme redirect 許可、cookie 経路の
    代替) が必須**になり、コストが跳ね上がる。

**結論**: 認証の観点からも **案A (リモート表示) が圧倒的に有利**。Tauri アプリは
「WebView2 に `https://alc.ippoan.org` を読ませる」ことに徹し、auth-worker の既存
認証フローに一切手を入れない方針を軸にする。Phase 0 で「WebView2 内で full-page
Google OAuth redirect + `.ippoan.org` cookie が成立するか」を必ず実機確認する
(WebView2 の third-party cookie / redirect 挙動が通常ブラウザと異なる可能性があるため)。

### 3.6 デバイス認証・登録

**追加実装は基本的に不要**。`useDeviceToken.ts` / QR・URL 登録ページは purely
web + localStorage + REST なので、Tauri の WebView 内でそのまま動く見込み。
確認するのは localStorage / cookie の永続化 (Tauri の WebView2 データディレクトリが
アプリ再起動をまたいで保持されるか = ログイン状態が再起動後も維持されるか) のみ。

### 3.6 起動形態: タスクバー常駐アプリ (system tray)

**Windows のタスクバー (通知領域 / system tray) に常駐するアプリとして起動する**
のが基本形態。フルスクリーンで画面を占有し続ける「ロックダウン型キオスク」ではなく、
**バックグラウンドに常駐しつつトレイアイコンから window を出し入れできる**モデル。

理由:
- NFC / FC-1200 / M5Stack (BLE) の各ブリッジ (`ws://127.0.0.1:*`) は **アプリが
  生きている間ずっと動いている必要がある**。トレイ常駐なら window を閉じても
  ブリッジは動き続けられる。
- 端末は「アルコールチェック専用端末」だが、OS を完全占有する Android の Device
  Owner ほどのロックダウンは Windows では過剰 / 運用が固い。トレイ常駐 + 自動起動で
  「常に裏で動いていて、必要な時に前面に出る」形が現実的。

Tauri 実装:
- `tauri` の **system tray** 機能でトレイアイコン + メニュー (「画面を開く」
  「再起動」「終了」等) を出す。
- ウィンドウの「×」は **終了せずトレイに最小化** (`prevent_close` で hide に倒す)。
  アプリ本体 (ブリッジ) はトレイに残り続ける。
- **Windows ログオン時に自動起動** (`tauri-plugin-autostart` or レジストリ Run キー)。
  ログオンしたら裏で常駐開始 → ブリッジ稼働。
- キオスク相当の「専用端末感」が要る運用では、window 表示時に `fullscreen` /
  `decorations:false` / devtools・右クリック無効化を **オプションで**被せられるように
  する (常時ロックダウンは強制しない)。
- 初期セットアップは手動 (インストール → トレイから window を出す → デバイス登録
  ページでペアリング)。QR での Device Owner プロビジョニング相当は Windows には無い。

### 3.7 配布 (GitHub Release) と自動アップデート

**配布は GitHub Release 経由で確定** (`rust-nfc-bridge` と同じ方式)。`v*` タグ push で
CI が Windows インストーラ (`.msi` / NSIS `.exe`) をビルドして GitHub Release に上げる。

- `tauri-plugin-updater` + GitHub Releases の `latest.json` manifest を使い、アプリが
  起動時 / 定期的に Release をチェック → 自動ダウンロード・再起動。
- updater には **署名が必須** (Tauri updater の公開鍵/秘密鍵)。秘密鍵は CI secret、
  公開鍵はアプリに埋め込む。鍵は `secret-inject` skill で GCP/GitHub に投入 (値を
  会話・log に出さない)。
- dev / prod チャネル分離が要るなら Android 版同様、`ci-workflows` の
  `dev-tag-release.yml` (dev, `dev-*` タグ) と `tag-release.yml` (prod, `v*` タグ) を
  使い、updater endpoint を channel ごとに出し分ける (要否は運用判断)。
- 署名は tauri updater の鍵であり、Windows の Authenticode コード署名 (SmartScreen
  警告回避) とは別。Authenticode 証明書の要否は別途判断 (無くても動くが警告が出る)。

### 3.8 CI/CD (ci-workflows の reusable を使用)

**CI は `ippoan/ci-workflows` の reusable workflow を使用する** (org 標準に揃える)。

- ただし ci-workflows の既存 reusable は **frontend (Nuxt/Worker) / Go / Node lib** 向けで、
  **Tauri (Rust + Windows インストーラビルド + GitHub Release) 用の reusable は現状無い**。
  よって以下のいずれか (要判断):
  - **A. ci-workflows に Tauri 用 reusable を新設** (`tauri-release.yml` 等) して本 repo が
    caller になる。他の Tauri アプリが増えた時に共有できる (org 標準化)。
  - **B. まず本 repo に bespoke workflow を置き**、`rust-nfc-bridge` の `release.yml`
    (windows-latest + cargo build + MSI 化 + Release アップロード) を下敷きにする。
    安定後に ci-workflows へ切り出す。
- caller 側 permissions は ci-workflows の規約に従う (`contents: write` 等、
  reusable の startup_failure 罠に注意 — `ci-workflows` の CLAUDE.md 参照)。
- PR ごとに `windows-latest` で build + test、`v*` タグで release ビルド + GitHub
  Release アップロード。auto-merge も ci-workflows の `auto-merge.yml` を踏襲。

### 3.9 遠隔点呼 (WebRTC)

Android は着信のためにバックグラウンド `RoomWatcher` + FCM full-screen intent が
必要だった (端末がロック/バックグラウンドになり得るため)。Windows はトレイ常駐 +
「必要時に window を前面化」する形なので、着信時にトレイからウィンドウを前面化する
ネイティブ連携が要る可能性はあるが、まずは **既存の `TenkoKiosk.vue` の `remoteMode`
prop によるページ内ポーリングだけで足りるか** を Phase 0 で確認 (window が hide 中でも
WebView 内 JS が動き続けるか = ポーリングが止まらないかに依存)。

## 4. フェーズ計画

| Phase | 内容 | 成果物 / 完了条件 |
|---|---|---|
| **0. 実機検証** | 最小 Tauri アプリで WebView2 の WebSerial / getDisplayMedia / getUserMedia / WebRTC / **full-page Google OAuth redirect + `.ippoan.org` cookie** 対応を確認 | 対応状況の一覧表 (3.1〜3.9 の設計判断がこれで確定する) |
| **1. 雛形構築** | `cargo tauri init`、`https://alc.ippoan.org` (or staging) を表示するだけの window。CI (`windows-latest` build) 疎通確認 | ビルド済み `.exe`/`.msi` が起動し alc-app のログイン/キオスク画面が出る |
| **2. NFC 統合** | `rust-nfc-bridge` sidecar 同梱・自動起動、実カードで `nfc_read` イベント疎通確認 | 実機で NFC タップ→Web側イベント受信 |
| **3. FC-1200 ネイティブブリッジ** | (Phase 0 結果次第) `fc1200-wasm` コアロジック流用の native bridge 実装、実センサーで疎通確認 | 実機で FC-1200 測定→Web側にデータ到達 |
| **4. BLE (M5Stack) ブリッジ** | M5Stack を USB シリアルで読む `ws://127.0.0.1:9877` ブリッジ実装 (FC-1200 と同型) | 実機で M5Stack 経由の体温/血圧→Web側にデータ到達 |
| **5. タスクバー常駐 UX** | system tray 常駐・×で最小化・ログオン自動起動・(オプション) fullscreen 化 | 実機で「ログオンしたら裏で常駐、トレイから window 出し入れ、再起動で自動復帰」 |
| **6. 認証・デバイス登録確認** | auth-worker Google OAuth ログイン + device pairing + localStorage/cookie 永続化確認 | 実機で再起動後もログイン/デバイス登録状態が保持される |
| **7. 自動アップデート** | `tauri-plugin-updater` 配線、dev/prod チャネル設計、CI release workflow | タグ push で `.msi`/`.exe` が Release に上がり、既存端末が自動更新される |
| **8. CI/CD** | `windows-latest` runner での build/test/release、`ci-workflows` 標準に揃える | PR ごとに build 検証、タグで自動リリース |
| **9. 実機ロールアウト** | 1台での実運用テスト → 展開 | 運用判断 (このリポジトリの scope 外) |

## 5. 主なリスク

- **WebView2 の Web API 対応状況が未確認** (Phase 0 で最優先に潰す。ここがボトルネック)。
  特に **full-page Google OAuth redirect + `.ippoan.org` cookie** が WebView2 で成立
  しないと、案A (リモート表示) の前提が崩れ auth-worker 改修が発生する。
- `fc1200-wasm` のコア分離 (wasm 依存を外す) が想定より大掛かりになる可能性
  → 最悪 native 版のプロトコル実装を素直に再実装 (Kotlin 版が既にあるので参考にはなる
    が、また3重目の実装になり避けたい)
- **ci-workflows に Tauri 用 reusable が無い** — 新設 (3.8 案A) か bespoke 先行 (案B) か
  の判断が要る。
- updater 署名鍵 / (任意) Authenticode 証明書の運用が未確定。
- 自動アップデートの配信チャネル運用 (dev/prod 分離の要否) が未確定。
- トレイ常駐で window hide 中に WebView 内 JS (WebRTC ポーリング等) が止まらないか未確認。

## 6. 確定した方針 (ユーザー確認済み)

- NFC = **PC/SC** (`rust-nfc-bridge` 流用)
- BLE = **Windows ネイティブ BLE は使わず M5Stack を USB シリアルで挟む** (信頼性懸念のため)
- 起動形態 = **タスクバー (system tray) 常駐アプリ**
- 認証 = **auth-worker** (Google OAuth → `.ippoan.org` cookie) をそのまま利用
- 配布 = **GitHub Release** 経由
- CI = **ci-workflows の reusable** を使用 (Tauri 用 reusable の新設要否は要判断)

## 7. 次のアクション

1. (要判断) 3.1 (案A で確定してよいか) / 3.3 (fc1200 コア分離) / 3.8 (reusable 新設 vs bespoke) の方針確定
2. Phase 0 (実機検証) から着手 — Windows 実機が無い場合は代替の検証手段を相談

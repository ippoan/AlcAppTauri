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
計 1600 行) は wasm-bindgen 向けだが、コアロジックは `wasm_bindgen` 依存を薄く分離できる。
方針:

1. `fc1200-wasm` 側でコアロジックを `#[cfg(feature = "wasm")]` で任意化し、素の Rust
   crate としても import 可能にする (要 `ippoan/fc1200-wasm` 側の改修、別 issue)。
   **分離コストは低い (Fable 裏取り)**: wasm-bindgen 依存は `src/lib.rs` の 11 箇所に
   完全隔離されており、`parser`/`state_machine`/`session`/`commands`/`modes`/`events` の
   6 モジュールは pure Rust + serde。`Cargo.toml` は既に `crate-type = ["cdylib","rlib"]`。
   feature gate 1 つで済み、体感 **半日以下**。リスク欄の「最悪プロトコル再実装」はほぼ杞憂。
2. `AlcAppTauri` 側に `fc1200-bridge` モジュールを新規実装:
   - `serialport` crate で COM ポート (9600bps / 8N1、README 記載の設定) を開く
   - `fc1200-wasm` のコアロジックで RS232C フレームを解釈
   - `ws://127.0.0.1:9878` で Android 版と同じ JSON プロトコルを喋る
     (`useFc1200Serial.ts` は無改修で動く)
   - **本当の工数の重心はここ (Fable 指摘)**: WebSerial 経路では JS が session を駆動するが、
     WS 経路では**ブリッジ側が session ループ + コマンド変換**を担う
     (`reset`/`sensor_lifetime`/`memory_read`/`memory_complete`/`date_update:*`/`connect`、
     および `status`/`permission_requested` 等の status イベント返し)。Kotlin の
     `Fc1200BridgeServer` / `Fc1200Protocol.kt` を contract 基準として、**JSON プロトコル
     互換テストを Phase 3 の完了条件に入れる** (フロント無改修の前提はこれで担保する)。
3. **トランスポート強制手段の必要性 (Fable H1)**: `useFc1200Serial.ts` のフォールバック
   判定は `'serial' in navigator` の**存在チェックのみ**。WebView2 は API オブジェクトが
   露出したまま chooser UI / broker が無く `requestPort()`/`getPorts()` が機能しない
   「present-but-broken」パターンが既知。この場合フォールバックが発動せず、**ブリッジを
   実装してもフロントが繋ぎに来ない**。Phase 0 で present-but-broken と判明したら、
   alc-app 側に **トランスポート強制手段** (query param / `NUXT_PUBLIC_*` env / UA 判定で
   WebSocket を強制) を追加する issue を起こす。この alc-app 改修は本移植の隠れた前提。
4. Phase 0 で WebSerial が**実際に機能する**と判明した場合のみ、この工程を **スキップ**して
   `fc1200-wasm` を wasm のまま WebView2 内で直接使う経路に倒せる。

**機密性の扱い**: `fc1200-wasm` は「プロトコル実装を秘匿するために WASM 化」という
経緯 (Tanita Confidential 資料に基づく実装のため)。ネイティブ化してもソース非公開の
private repo 依存に変わりはなく、コンパイル済みバイナリの解析難易度は wasm と大差ない
という理解で進めるが、懸念があれば要相談。**CI 配線の前提 (Fable M4)**: fc1200-wasm は
private repo なので、AlcAppTauri の CI (windows-latest) が git dependency で pull するには
トークン (deploy key / CI App token) の配線が要る。3.8 の reusable 議論より先に詰める。

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
  WebSerial 直結に倒せる (FC-1200 と同じ判断)。ただし present-but-broken の場合の
  トランスポート強制手段 (3.3 項3 = alc-app 改修) が BLE 側にも同様に必要。

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

**Google の embedded WebView ブロック (Fable H2、重大)**: Google OAuth は「埋め込み
WebView」を `disallowed_useragent` で弾くポリシーがある。Android 版は
`WebViewActivity.kt` で `userAgentString.replace("; wv", "")` と **UA を偽装して回避した
前歴**がある。WebView2 の UA は Edge と同一なので通る公算は高いが、**Phase 0 で必ず
実機確認**し、blocked の場合の fallback (WebView2 の UA override / 外部ブラウザ + loopback
redirect) まで用意する。ここが通らないと案A の前提が崩れる。

**結論**: 認証の観点からも **案A (リモート表示) が圧倒的に有利**。Tauri アプリは
「WebView2 に `https://alc.ippoan.org` を読ませる」ことに徹し、auth-worker の既存
認証フローに一切手を入れない方針を軸にする。Phase 0 で「WebView2 内で full-page
Google OAuth redirect + `.ippoan.org` cookie + Google の WebView ブロック回避」が
成立するかを必ず実機確認する。

なお **device 認証も案B では壊れる (Fable 裏取り)**: `useDeviceToken.ts` の device JWT は
alc-app の server proxy `/api/proxy/*` (Cloudflare Workers 上の Nuxt server route) に
依存しており、静的バンドルには server route が無い。案B は「相性が良いか要確認」ではなく
**事実上不成立**、と案A の根拠が補強される。

### 3.6 デバイス認証・登録 + device token (キオスク運用の中核)

キオスク端末は「人がログインし続ける」運用ではなく、**端末そのものをテナントに紐付けて
無人で使い続ける**のが本筋。よって device token 経路がキオスク運用の中核になる。

**device token の流れ** (`useDeviceToken.ts` 実測、追加実装は基本不要):
1. 初回セットアップ: auth-worker `/device/pair` (role=`device-kiosk`) で pairing →
   `device_id` + `device_secret` を localStorage に保存 (secret は auth-worker 側で
   hash 保存・再取得不可 = 1 度だけ)。
2. 運用時: `device_secret` を auth-worker `/device/token` に提示し **短命 device JWT (1h)**
   を mint。expiry 60s 手前まで cache 再利用。
3. その JWT を `Authorization: Bearer` で alc-app の server proxy `/api/proxy/*`
   (Cloudflare Workers 上の Nuxt server route) に送る。
4. credential 無し / mint 失敗時は従来の `X-Tenant-ID` 経路に fallback (段階移行で非破壊)。

**credential の永続化先: localStorage ではなくネイティブファイルを真実に (要検討・重要)**

現状の `useDeviceToken.ts` は `device_id`/`device_secret` を **WebView の localStorage**
に保存する。しかし WebView2 の localStorage は **WebView2 のユーザーデータフォルダに
紐付く揮発しやすいストア**で、以下で消える:
- アプリ再インストール / updater によるデータディレクトリ差し替え
- ユーザーデータパスの変更、プロファイル破損時のリセット

消えると無人キオスクが**再ペアリング要求で停止**する = 運用事故。よって:

- **Android 版は元々ネイティブ側 (`device_settings` SharedPreferences = 実質ファイル)
  に device_id 等を保存していた** (AlcoholChecker の SharedPreferences)。web の
  localStorage は別系統のキャッシュに過ぎない。**「credential の真実はネイティブの
  ファイル、localStorage はミラー」という Android の設計を Windows でも踏襲すべき**。
- Windows 実装: Tauri 側で credential を **AppData 配下のファイル** (`tauri-plugin-store`
  or 平ファイル、可能なら DPAPI 等で暗号化) に保存し、これを真実とする。

**案A (リモート表示) 特有の橋渡し問題**: WebView が remote origin (`alc.ippoan.org`) を
読むため、web ↔ ネイティブファイル間に直接の Tauri IPC が無い (remote origin には
Tauri API が注入されない)。橋渡し手段を Phase 0/6 で決める:
1. **起動時シード**: Tauri の `initialization_script` でネイティブファイルの credential を
   読んで `localStorage` に seed してからページを読む (読み取り方向)。
2. **書き戻し**: pairing が web 側で起きた時にネイティブファイルへ保存する経路。
   ローカル WS 制御チャネル (既存ブリッジと同じ 127.0.0.1) or Tauri v2 の remote
   capability で web → native を通す。
3. あるいは **credential を丸ごとネイティブ (Tauri コマンド/ローカル HTTP) 側に持たせ、
   web は毎回そこから取得**する案 (localStorage を使わない)。ただし alc-app 改修が要る。

→ 「localStorage 依存のまま WebView2 データdirを安定させる」か「ネイティブファイルを
真実にして橋渡しする」かは、**Phase 0 の検証 (localStorage が updater/再起動で残るか) の
結果を見て決める**。無人キオスクの堅牢性を取るなら後者 (ネイティブファイル) を推す。

**その他のキオスク運用ポイント**:
- 端末登録は QR / URL / コード入力の3フロー (`DeviceRegistration.vue` /
  `device-claim.vue` / `device-approve.vue`)。Windows では QR スキャンではなく
  **管理者が URL/コードを端末に入力**するのが現実的 (Android の Device Owner QR
  プロビジョニング相当は Windows に無い)。
- staging では `NUXT_PUBLIC_STAGING_TENANT_ID` の auth バイパス (`X-Tenant-ID` 直送)
  も使える。実機検証を軽くするのに有効。
- device token は Google ログイン (3.5) と**別系統で人手ログイン不要**。キオスクの
  既定運用はこちらに寄せ、Google OAuth は管理者操作時のみ、という切り分けを想定。

これらは案A 成立性そのものに直結するので **Phase 0 で cookie/localStorage 永続化 +
ネイティブファイル橋渡しの要否を確認**する (Fable L1)。

### 3.7 起動形態: タスクバー常駐 + キオスク運用

**Windows のタスクバー (通知領域 / system tray) に常駐するアプリとして起動する**
のが基本形態。ただし用途は「アルコールチェック専用端末」なので、**キオスク運用
(専用端末感・誤操作防止) も視野に入れた二層構成**にする:

- **常駐層**: バックグラウンドに常駐し各ブリッジ (`ws://127.0.0.1:*`) を動かし続ける。
  window を閉じてもブリッジは生存。
- **キオスク層 (オプション)**: window 表示時に fullscreen / chrome 無し / 誤操作防止を
  被せ、専用端末として使わせる。常駐は保ったまま「見た目はキオスク」を実現する。

理由:
- NFC / FC-1200 / M5Stack (BLE) の各ブリッジは **アプリが生きている間ずっと動いて
  いる必要がある**。トレイ常駐なら window を閉じてもブリッジは動き続けられる。
- OS を完全占有する Android の Device Owner ほどの強制ロックダウンは Windows では
  過剰。トレイ常駐 + 自動起動 + (必要なら) fullscreen キオスク表示、の組み合わせで
  「常に裏で動いていて、必要な時に専用端末画面を前面に出す」形が現実的。

Tauri 実装:
- `tauri` の **system tray** 機能でトレイアイコン + メニュー (「画面を開く」
  「再起動」「終了」等)。終了はメニューからのみ (誤終了防止)。
- ウィンドウの「×」は **終了せずトレイに最小化** (`prevent_close` で hide に倒す)。
- **Windows ログオン時に自動起動** (`tauri-plugin-autostart` or レジストリ Run キー)。
- **キオスクモード (オプション、設定で ON)**: 表示時に `fullscreen: true` /
  `decorations: false` / devtools・右クリック・キーボードショートカット (Alt+F4 等)
  無効化。専用端末運用ではこれを既定 ON にする。
- **single-instance 強制 (Fable L2)**: `tauri-plugin-single-instance` で 2 個目の起動を
  抑止する。無いと 2 プロセスが COM ポート / WS ポートを取り合って壊れる。
- **起動時ネット未確立への耐性 (Fable M5)**: autostart はログオン直後 =
  ネットワーク確立前に走りがちで、リモート URL (`alc.ippoan.org`) の初回ロードが
  失敗しうる。ロード失敗時の **自動リトライ / 再ナビゲーション** を実装する
  (無人キオスクでは「読み込み失敗のまま放置」が致命的)。
- **ブリッジ起動順序の保証 (Fable M2)**: `useFc1200Serial.ts` は WS 再接続を
  `10 回 × 3s ≈ 30s` で永久に諦める。sidecar/ブリッジがページロードより遅く立つと
  復帰しない。**Tauri 側でブリッジの listen 確認後に navigation する** 等の順序保証を
  Phase 3/4 の設計に入れる。
- **カメラ権限の永続化 (Fable M6)**: WebView2 の `PermissionRequested` を Tauri がどう
  処理するか次第で **毎起動プロンプト**が出ると無人キオスクで詰む。Windows のカメラ/
  マイク プライバシートグルも desktop app に効く。再起動を跨いで権限が保持されるか
  Phase 0 で確認する。
- 初期セットアップは手動 (インストール → トレイから window → デバイス登録ページで
  URL/コード入力ペアリング)。QR での Device Owner プロビジョニング相当は Windows に無い。

### 3.8 配布 (GitHub Release) と自動アップデート

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
- **インストーラ形式 (Fable L4)**: Tauri updater は MSI と NSIS で挙動が異なり、**NSIS の
  方が updater と相性が良い**。Phase 7 前に形式を確定する。WebView2 Evergreen Runtime
  ブートストラップの同梱要否 (オフライン端末対策) も合わせて決める。

### 3.9 CI/CD (ci-workflows の reusable を使用)

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

### 3.10 遠隔点呼 (WebRTC) と hidden window の JS throttling

Android は着信のためにバックグラウンド `RoomWatcher` + FCM full-screen intent が
必要だった (端末がロック/バックグラウンドになり得るため)。Windows はトレイ常駐 +
「必要時に window を前面化」する形。

**hidden window の JS throttling (Fable M1、重要)**: Chromium は hidden ページの
タイマーを throttle する (5 分経過後は intensive throttling で概ね 1 回/分)。
**WebSocket の `onmessage` 自体は届く**が、**再接続の `setTimeout` やポーリングの
`setInterval` は激遅化**する。つまり:
- 各ブリッジ WebSocket は受信し続けるが、切断→再接続が激遅になる。
- `TenkoKiosk.vue` の `remoteMode` ページ内ポーリング (着信検知) が hide 中に止まる恐れ。

緩和策: Tauri の `windows.additionalBrowserArgs` で
`--disable-background-timer-throttling --disable-features=IntensiveWakeUpThrottling`
を渡す。**「window を 10 分 hide → ポーリング/WS 再接続が生きているか」を Phase 0 の
完了条件に含める**。着信時の window 前面化は、案A では remote origin に Tauri IPC が
無いため、**ローカル WS 制御チャネル** or Tauri v2 の remote capability 設定で実現する
(具体化は Phase 5)。

## 4. フェーズ計画

| Phase | 内容 | 成果物 / 完了条件 |
|---|---|---|
| **0. 実機検証** | 最小 Tauri アプリで WebView2 の各 Web API を **presence でなく機能性で**検証 (下記マトリクス) | 検証マトリクスを plan に追記 + fail 時にどの Phase が増減するかの gate 表 (Fable L1)。3.1/3.3/3.9 の設計を確定 |
| **1. 雛形構築** | `cargo tauri init`、`https://alc.ippoan.org` (or staging) を表示するだけの window + 起動時ロード失敗リトライ | ビルド済みインストーラが起動し alc-app のログイン/キオスク画面が出る (CI `windows-latest` build 疎通) |
| **2. NFC 統合** | `rust-nfc-bridge` sidecar 同梱・自動起動、実カードで `nfc_read` 疎通 | 実機で NFC タップ→Web側イベント受信 |
| **3. FC-1200 ネイティブブリッジ** | `fc1200-wasm` コアロジック流用の native bridge。**Kotlin `Fc1200BridgeServer` を contract にした JSON プロトコル互換テスト**。(present-but-broken なら alc-app のトランスポート強制 issue も) | 実機で FC-1200 測定→Web側到達 + プロトコル互換テスト green |
| **4. BLE (M5Stack) ブリッジ** | M5Stack を USB シリアルで読む `ws://127.0.0.1:9877` ブリッジ (FC-1200 と同型)。**USB 抜き差し/スリープ復帰の再オープン (Fable L5)** | 実機で M5Stack 経由の体温/血圧→Web側到達、抜き差し後も復帰 |
| **5. タスクバー常駐 + キオスク UX** | system tray 常駐・×で最小化・ログオン自動起動・single-instance・(オプション)fullscreenキオスク・着信時 window 前面化・ブリッジ起動順序保証 | 実機で「ログオンで裏常駐、トレイ出し入れ、再起動で自動復帰、キオスク表示」 |
| **6. 認証・device token・登録確認** | auth-worker Google OAuth + **device pairing→device JWT→`/api/proxy` (キオスク中核)** + credential 永続化 (**localStorage が脆いならネイティブファイルを真実に + 案A 橋渡し**) + cookie/カメラ権限の永続化 | 実機で再起動 + updater 適用後も device credential/ログイン/権限が保持され無人運用できる |
| **7. 配布 (GitHub Release) + 自動アップデート** | `tauri-plugin-updater` 配線 (NSIS 推奨)、署名鍵投入、dev/prod チャネル設計、release workflow | タグ push でインストーラが Release に上がり既存端末が自動更新 |
| **8. CI/CD** | `windows-latest` build/test/release、`ci-workflows` reusable、**fc1200-wasm private repo pull トークン配線** | PR ごとに build 検証、タグで自動リリース |
| **9. 実機ロールアウト** | 1台での実運用テスト → 展開 | 運用判断 (このリポジトリの scope 外) |

### Phase 0 検証マトリクス (presence / 機能性の 2 軸、Fable H1)

| API / 挙動 | 確認内容 | fail 時の影響 |
|---|---|---|
| `navigator.serial` | 存在だけでなく `requestPort()`/`getPorts()` が**実際に動くか** (present-but-broken 検出) | 動かない → FC-1200/BLE ブリッジ必須 + alc-app にトランスポート強制手段 (3.3項3) |
| `getUserMedia()` | カメラ取得 + Windows カメラ権限プロンプト挙動 + **権限が再起動を跨いで保持されるか** | 権限が毎回 → 無人キオスクで詰む (3.7 カメラ権限) |
| `getDisplayMedia()` | 画面共有が WebView2 で動くか | **fail 率高 (H3)**: Windows 版でネイティブキャプチャ代替を作る or 画面共有を scope 外宣言、を判断 |
| WebRTC | P2P 通話成立 | 遠隔点呼の管理者通話に影響 |
| Google OAuth redirect | full-page redirect + `.ippoan.org` cookie + **embedded WebView ブロック回避 (H2)** | 案A の前提崩壊 → auth-worker 改修 or UA override |
| device token 永続化 | `device_id`/`device_secret` localStorage が **再起動 + updater 適用 + 再インストール**で残るか | 消えるならネイティブファイルを真実にする (3.6、案A の橋渡しが必要) |
| hidden window throttling | window 10 分 hide 後もポーリング/WS 再接続が生きるか (M1) | browser args で緩和 (3.10) |
| loopback `ws://127.0.0.1` | mixed content で許可されるか (L3) | (desktop Chrome で実績あり、確認のみ) |

## 5. 主なリスク (Fable レビュー反映済み)

優先度は Fable の指摘レベルに対応。

- **[High] WebSerial が present-but-broken (H1)** — WebView2 で `navigator.serial` が
  露出したまま機能せず、既存フォールバック判定 (`'serial' in navigator`) をすり抜けて
  ブリッジに繋がらない。alc-app 側のトランスポート強制改修が隠れ前提。
- **[High] Google OAuth の embedded WebView ブロック (H2)** — `disallowed_useragent`。
  UA override / 外部ブラウザ fallback を Phase 0 で用意。案A の生死に直結。
- **[High] `getDisplayMedia()` の WebView2 非対応リスク (H3)** — fail 率高。fail 時に
  「ネイティブキャプチャ代替 (大工事)」か「画面共有を Windows 版 scope 外」かの
  判断ポイントをフェーズ表に明記済み。
- **[Med] hidden window の JS throttling (M1)** — WS 受信は生きるが再接続/ポーリングが
  激遅化。browser args で緩和 (3.10)。
- **[Med] ブリッジ起動順序 × 再接続上限 30s (M2)** — 起動順序保証が要る。
- **[Med] fc1200-wasm が private repo → CI pull トークン配線 (M4)** — reusable 議論の前に詰める。
- **[Med] 起動時ネット未確立 (M5)** / **カメラ権限の毎回プロンプト (M6)** — 無人運用の穴。
- **ci-workflows に Tauri 用 reusable が無い** — 新設 (3.9 案A) か bespoke 先行 (案B) か判断要。
- updater 署名鍵 / (任意) Authenticode / 配信チャネル (dev/prod) / インストーラ形式
  (NSIS 推奨) が未確定。

> **fc1200-wasm のコア分離は Fable 裏取りで「低コスト (半日以下)」と判明** — wasm-bindgen
> 依存は `src/lib.rs` の 11 箇所に隔離済み。旧「最悪プロトコル再実装」の悲観は撤回。
> 工数の重心は WS ブリッジの **プロトコル互換** (3.3 項2) に移す。

## 6. 確定した方針 (ユーザー確認済み)

- NFC = **PC/SC** (`rust-nfc-bridge` 流用)
- BLE = **Windows ネイティブ BLE は使わず M5Stack を USB シリアルで挟む** (信頼性懸念のため)
- 起動形態 = **タスクバー (system tray) 常駐アプリ + キオスク運用も視野**
- 認証 = **auth-worker** (Google OAuth → `.ippoan.org` cookie) をそのまま利用。
  キオスク無人運用は **device token** (`/device/pair` → `/device/token` → `/api/proxy`) を中核に
- 配布 = **GitHub Release** 経由
- CI = **ci-workflows の reusable** を使用 (Tauri 用 reusable の新設要否は要判断)

## 7. 次のアクション

1. (要判断) 3.1 (案A で確定してよいか) / 3.3 (fc1200 コア分離) / 3.8 (reusable 新設 vs bespoke) の方針確定
2. Phase 0 (実機検証) から着手 — Windows 実機が無い場合は代替の検証手段を相談

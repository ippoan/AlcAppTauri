# Phase 7 — 配布 + 自動アップデート

## 何ができるようになったか

1. **tauri-plugin-updater** を有効化 (`src-tauri/Cargo.toml` + `tauri.conf.json`)
2. 起動時と 1 時間おきに endpoint (`dev-current` moving Release) を polling し、
   新版が出ていれば **installMode=passive** で無人インストール + 自動再起動
3. CI (`ci.yml`) で:
   - `tauri.conf.json` の version を tag 由来に自動 patch (dev-N → `0.0.N`、
     `v*` → セマンティック version、それ以外は `0.0.<run_number>`)
   - `TAURI_SIGNING_PRIVATE_KEY` を build 時に注入して署名成果物を生成
   - `latest.json` (updater schema) を jq で組み立て、NSIS + `.sig` と一緒に
     Release にアップロード
   - `dev-current` tag / Release を毎回上書きして dev channel の
     "最新" URL を安定化
4. tray メニューに **「ログをコピー」** を追加。直近 2000 行を
   クリップボードにコピーする (WS ハブに繋げない環境での診断路)

## 初回セットアップ (user が 1 回だけ実行)

`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` が
未投入の間は CI が `updater を無効化した config で build` に fallback するので、
NSIS 自体は出るが auto-update は動かない。以下で有効化する。

### 1. 署名鍵ペアの生成 (手元マシンで)

```sh
# tauri CLI が入っていなければ
npm i -g @tauri-apps/cli

# password 付きの keypair を生成
tauri signer generate -w ~/.tauri/alcapp.key
```

- `~/.tauri/alcapp.key` = 秘密鍵 (PKCS#8 PEM 相当のテキスト)
- `~/.tauri/alcapp.key.pub` = 公開鍵 (テキスト)

### 2. 公開鍵を `tauri.conf.json` に埋め込む

```sh
PUB=$(cat ~/.tauri/alcapp.key.pub)
# tauri.conf.json の "REPLACE_WITH_TAURI_SIGNER_PUBKEY" を PUB で置換
# 空白改行含めコピペミス防止のため sed/jq 経由が安全
jq --arg p "$PUB" '.plugins.updater.pubkey = $p' src-tauri/tauri.conf.json \
  > tmp && mv tmp src-tauri/tauri.conf.json
```

置換後の `tauri.conf.json` を PR で commit する (公開鍵は git tracked、
リークしても検証にしか使えないので問題なし)。

### 3. 秘密鍵を GitHub Actions secret に投入 (`secret-inject`)

CLAUDE.md 規範に沿い、値を LLM context / plain env / tool-call param に
一切載せずに GitHub Actions secret へ流す:

```sh
# TAURI_SIGNING_PRIVATE_KEY
cat ~/.tauri/alcapp.key | bash ~/.claude/skills/secret-inject/scripts/inject-secret.sh \
  TAURI_SIGNING_PRIVATE_KEY --targets github --repo ippoan/AlcAppTauri

# TAURI_SIGNING_PRIVATE_KEY_PASSWORD (`tauri signer generate` 時に打った password)
printf '%s' 'YOUR_PASSWORD_HERE' | bash ~/.claude/skills/secret-inject/scripts/inject-secret.sh \
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD --targets github --repo ippoan/AlcAppTauri
```

投入後、次の PR / main push で updater 有効な signed build が出る。

## 動作確認

1. PR を出す or main push
2. CI で `release-dev` job が: NSIS 署名 → `latest.json` 生成 →
   `dev-N` Release + `dev-current` Release 更新
3. **既に前版 (dev-K, K<N) を install している端末**を再起動 or 1h 待つ
4. 起動時の polling で `dev-current/latest.json` を見て新版検出
5. passive install で NSIS 実行 → 完了時に app.restart() で自動再起動
6. tray 右クリック → 「ログをコピー」で updater ログを取り出せる

## dev/prod チャネル分離

現在は **dev channel のみ**。stable (`v*`) は `v*` タグを実際に切る
タイミングで別 PR で対応:

- `release-stable` job にも同じ latest.json 生成 + アップロードを追加
- `tauri.conf.json` の `plugins.updater.endpoints` に stable 用 URL を追加
  (例: `https://github.com/ippoan/AlcAppTauri/releases/latest/download/latest.json`)
- 起動時に "自分の version が prerelease (0.0.N) か stable (X.Y.Z) か" で
  チャネル選択、はしない (tauri-plugin-updater は endpoints リストを順に
  試すだけ)。運用で dev 端末 / prod 端末の install 元を分離する方が単純

## updater が device credential を消さない件 (Phase 6 前提)

`installMode=passive` の NSIS はアップグレード上書きで app data
(AppData\Roaming\org.ippoan.alcapp\*) を保持する。localStorage は
webview2 の user data folder (WebView2 が独立に管理) に置かれる。
Phase 6 で **credential をネイティブファイル (%APPDATA%\alcapp\device.key)
に真実化**しても updater は消さない (NSIS はアンインストールしない限り
app data を触らない設計)。実機で再起動 + updater 適用後の credential
保持は Phase 6 完了条件として実測する。

## WebView2 Runtime ブートストラップ

Tauri v2 の NSIS bundle は既に WebView2 Runtime の bootstrapper を同梱する
default 設定 (`webviewInstallMode: "downloadBootstrapper"`)。offline install
が要る場合のみ `"embedBootstrapper"` に切り替え検討。

## Refs

- Epic issue: #1 (Phase 7)
- 動機と設計判断: `plan/00-tauri-windows-migration.md`

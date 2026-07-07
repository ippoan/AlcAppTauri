//! ネイティブ層 (Rust) ログの 127.0.0.1 WebSocket ハブ。
//!
//! 目的: 実機 (WebView2) で Tauri ネイティブ層 (tray / 起動リトライ / 将来の
//! シリアルブリッジ等) が何をしているかを、別ブラウザ・別ウィンドウから
//! `ws://127.0.0.1:<port>` に繋いで確認できるようにする開発支援。
//!
//! 設計:
//! - `tracing` の出力を `MakeWriter` 経由で `broadcast` チャネルに流し込み、
//!   stdout にも並行して出す (`MakeWriterExt::and`)。
//! - WS 接続ごとに `broadcast::Receiver` を subscribe し、以後のログ行を push。
//! - **bind は 127.0.0.1 固定** (外部到達不可)。閲覧 UI は alc-app 側 (dev
//!   ログビューア) が担当し、この crate は「ハブ」までしか持たない (薄く保つ)。
//! - bind 失敗は warn ログのみで握り潰す (無人キオスクを落とさない fail-open)。

use std::io::{self, Write};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tracing_subscriber::fmt::writer::MakeWriterExt;

/// ブロードキャストのバッファ長。閲覧側が遅くても直近ログは追える程度。
const CHANNEL_CAPACITY: usize = 1024;

/// ログ行を全 WS 購読者へ配る中枢。`clone` は同一チャネルを共有する。
#[derive(Clone)]
pub struct LogHub {
    tx: broadcast::Sender<String>,
}

impl Default for LogHub {
    fn default() -> Self {
        Self::new()
    }
}

impl LogHub {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
        Self { tx }
    }

    fn sender(&self) -> broadcast::Sender<String> {
        self.tx.clone()
    }
}

/// `tracing` fmt layer の出力先。整形済みの 1 イベント分バイト列を受け取り、
/// UTF-8 として broadcast へ送る。購読者ゼロでも `send` はエラーにならない
/// 実装 (`broadcast::Sender::send` は receiver 不在時 `Err` を返すが握り潰す)。
struct BroadcastWriter {
    tx: broadcast::Sender<String>,
}

impl Write for BroadcastWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(s) = std::str::from_utf8(buf) {
            // 末尾改行はビューア側の行区切りに任せるため trim せずそのまま送る。
            let _ = self.tx.send(s.to_string());
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
struct BroadcastMakeWriter {
    tx: broadcast::Sender<String>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BroadcastMakeWriter {
    type Writer = BroadcastWriter;

    fn make_writer(&'a self) -> Self::Writer {
        BroadcastWriter {
            tx: self.tx.clone(),
        }
    }
}

/// `tracing` subscriber を初期化する。stdout と WS ハブの両方へ出力。
/// フィルタは `ALC_LOG` env (未設定なら `info`)。多重初期化は `try_init` で無害化。
pub fn init_tracing(hub: &LogHub) {
    let filter = tracing_subscriber::EnvFilter::try_from_env("ALC_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let bmw = BroadcastMakeWriter { tx: hub.sender() };
    let writer = std::io::stdout.and(bmw);
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(true)
        .with_writer(writer)
        .try_init();
}

/// 127.0.0.1:`port` で WS ハブを待ち受ける。接続ごとに以後のログ行を push。
/// この関数は「ずっと accept し続ける」ため `async_runtime::spawn` から呼ぶ。
pub async fn serve(hub: LogHub, port: u16) {
    let addr = format!("127.0.0.1:{port}");
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            // 既に別インスタンスが握っている等。落とさず warn のみ。
            tracing::warn!(error = %e, %addr, "log ws: bind failed (hub disabled)");
            return;
        }
    };
    tracing::info!(%addr, "log ws: hub listening (native-layer logs)");

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(error = %e, "log ws: accept failed");
                continue;
            }
        };
        let mut rx = hub.sender().subscribe();
        tokio::spawn(async move {
            let ws = match tokio_tungstenite::accept_async(stream).await {
                Ok(ws) => ws,
                Err(e) => {
                    tracing::debug!(error = %e, %peer, "log ws: handshake failed");
                    return;
                }
            };
            let (mut write, mut read) = ws.split();
            loop {
                tokio::select! {
                    line = rx.recv() => match line {
                        Ok(line) => {
                            if write.send(Message::Text(line.into())).await.is_err() {
                                break; // 相手が切断
                            }
                        }
                        // 遅い購読者で溢れた分はスキップして継続。
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                    // 相手からの close / エラーを検知して掃除する。
                    incoming = read.next() => match incoming {
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(_)) => break,
                        _ => {}
                    },
                }
            }
        });
    }
}

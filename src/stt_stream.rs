//! Opt-in, per-utterance upload. Local endpointing owns finalization; incomplete
//! remote results never become prompts. Dropping a stream closes its connection.
use anyhow::{bail, ensure, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::Duration;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

pub struct Live {
    sender: Option<mpsc::Sender<Vec<u8>>>,
    result: Option<Transcript>,
    sent: usize,
}
pub struct Transcript {
    task: JoinHandle<Result<String>>,
}
impl Drop for Transcript {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Transcript {
    pub async fn text(mut self) -> Result<String> {
        (&mut self.task)
            .await
            .context("Streaming transcription stopped")?
    }
}
impl Live {
    pub fn start(runtime: &tokio::runtime::Handle) -> Self {
        let (sender, receiver) = mpsc::channel(128);
        let task = runtime.spawn(async move {
            let key = crate::config::secret("cartesia", "CARTESIA_API_KEY")?;
            let mut request = "wss://api.cartesia.ai/stt/websocket?model=ink-2&encoding=pcm_s16le&sample_rate=16000&language=en".into_client_request()?;
            request.headers_mut().insert("X-API-Key", key.parse()?);
            request.headers_mut().insert("Cartesia-Version", "2026-08-14".parse()?);
            tokio::time::timeout(Duration::from_secs(45), exchange(request, receiver))
                .await.context("Streaming transcription timed out")?
        });
        Self {
            sender: Some(sender),
            result: Some(Transcript { task }),
            sent: 0,
        }
    }
    /// Receives the growing local utterance, including pre-roll. Never blocks DSP.
    pub fn update(&mut self, samples: &[f32], final_chunk: bool) {
        while samples.len().saturating_sub(self.sent) >= 1600
            || (final_chunk && self.sent < samples.len())
        {
            let end = (self.sent + 1600).min(samples.len());
            let bytes: Vec<u8> = samples[self.sent..end]
                .iter()
                .flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
                .collect();
            self.sent = end;
            if self
                .sender
                .as_ref()
                .is_some_and(|tx| tx.try_send(bytes).is_err())
            {
                self.sender = None;
                if let Some(result) = &self.result {
                    result.task.abort();
                }
                break;
            }
        }
    }
    pub fn finish(mut self, samples: &[f32]) -> Transcript {
        self.update(samples, true);
        self.sender = None;
        self.result.take().expect("stream has one result")
    }
}

async fn exchange(
    request: tokio_tungstenite::tungstenite::http::Request<()>,
    mut audio: mpsc::Receiver<Vec<u8>>,
) -> Result<String> {
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(4),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .context("Streaming connection timed out")??;
    let (mut send, mut receive) = socket.split();
    let mut finalized = false;
    let deadline = tokio::time::sleep(Duration::from_secs(45));
    tokio::pin!(deadline);
    let mut text = String::new();
    loop {
        tokio::select! {
            _=&mut deadline=>bail!("Streaming transcript did not finish"),
            chunk=audio.recv(), if !finalized=> {
                if let Some(chunk)=chunk {send.send(Message::Binary(chunk.into())).await?;}
                else {
                    send.send(Message::Text("finalize".into())).await?;
                    finalized=true;
                    deadline.as_mut().reset(tokio::time::Instant::now()+Duration::from_secs(8));
                }
            },
            message=receive.next()=> {
                match message.context("Streaming connection closed before final transcript")?? {
                    Message::Text(raw)=> {
                        let event:Value=serde_json::from_str(&raw)?;
                        match event["type"].as_str() {
                            Some("transcript") if event["is_final"]==true=> {
                                // Manual-finalization transcripts are deltas, not snapshots.
                                if let Some(delta)=event["text"].as_str() {
                                    ensure!(text.len()+delta.len()<32_000,"Streaming transcript too long");
                                    if !text.is_empty() && !delta.starts_with(char::is_whitespace) {text.push(' ');}
                                    text.push_str(delta);
                                }
                            },
                            Some("flush_done") if finalized=> {
                                ensure!(!text.trim().is_empty(),"Streaming returned no words");
                                let _=send.close().await;
                                return Ok(text.trim().into());
                            },
                            Some("error")=>bail!("Cartesia streaming reported an error"),
                            _=>{},
                        }
                    },
                    Message::Ping(data)=>send.send(Message::Pong(data)).await?,
                    Message::Close(_)=>bail!("Streaming closed before finalization"),
                    _=>{},
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn uploads_before_endpoint_and_returns_all_final_deltas() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Binary(_)
            ));
            observed_tx.send(()).unwrap();
            ws.send(Message::Text(
                r#"{"type":"transcript","is_final":false,"text":"wrong interim"}"#.into(),
            ))
            .await
            .unwrap();
            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Text("finalize".into())
            );
            for message in [
                r#"{"type":"transcript","is_final":true,"text":"first phrase"}"#,
                r#"{"type":"transcript","is_final":true,"text":"and continuation"}"#,
                r#"{"type":"flush_done"}"#,
            ] {
                ws.send(Message::Text(message.into())).await.unwrap();
            }
        });
        let (tx, rx) = mpsc::channel(4);
        let worker = tokio::spawn(exchange(url.into_client_request().unwrap(), rx));
        tx.send(vec![0; 3200]).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), observed_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(!worker.is_finished(), "interim text escaped as a prompt");
        drop(tx);
        assert_eq!(
            worker.await.unwrap().unwrap(),
            "first phrase and continuation"
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn growing_capture_uploads_each_sample_once_and_keeps_preroll() {
        let (sender, mut rx) = mpsc::channel(16);
        let task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            while let Some(chunk) = rx.recv().await {
                bytes.extend(chunk);
            }
            Ok(format!(
                "{}:{}",
                bytes.len(),
                bytes.iter().map(|v| *v as u64).sum::<u64>()
            ))
        });
        let mut live = Live {
            sender: Some(sender),
            result: Some(Transcript { task }),
            sent: 0,
        };
        let samples: Vec<f32> = (0..4300).map(|n| n as f32 / 4300.0).collect();
        live.update(&samples[..900], false);
        live.update(&samples[..2000], false);
        live.update(&samples[..3200], false);
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|v| ((v * 32767.0) as i16).to_le_bytes())
            .collect();
        assert_eq!(
            live.finish(&samples).text().await.unwrap(),
            format!(
                "{}:{}",
                bytes.len(),
                bytes.iter().map(|v| *v as u64).sum::<u64>()
            )
        );
    }

    #[tokio::test]
    async fn disconnect_does_not_return_partial_text() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            ws.send(Message::Text(
                r#"{"type":"transcript","is_final":true,"text":"incomplete"}"#.into(),
            ))
            .await
            .unwrap();
            ws.close(None).await.unwrap();
        });
        let (_tx, rx) = mpsc::channel(4);
        assert!(exchange(url.into_client_request().unwrap(), rx)
            .await
            .is_err());
        server.await.unwrap();
    }
}

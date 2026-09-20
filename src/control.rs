//! Session-bound, authenticated loopback bridge for MCP live controls.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Endpoint {
    address: SocketAddr,
    token: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Sleep,
    StopAlarm,
    Status,
}
pub struct Request {
    pub action: Action,
    pub reply: oneshot::Sender<Value>,
}
pub struct Bridge {
    pub endpoint: Endpoint,
    task: tokio::task::JoinHandle<()>,
    path: PathBuf,
}
impl Bridge {
    pub async fn start(input: mpsc::Sender<crate::Input>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = Endpoint {
            address: listener.local_addr()?,
            token: uuid::Uuid::new_v4().to_string(),
        };
        let directory = crate::config::home()?.join("sessions");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{}.json", uuid::Uuid::new_v4()));
        crate::config::save_private(&path, &serde_json::to_vec(&endpoint)?)?;
        let auth = endpoint.token.clone();
        let task = tokio::spawn(async move {
            // A bounded set of short-lived requests cannot starve audio or grow unbounded.
            let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let Ok(slot) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                let input = input.clone();
                let auth = auth.clone();
                tokio::spawn(async move {
                    let _slot = slot;
                    let _ =
                        tokio::time::timeout(Duration::from_secs(4), handle(stream, &auth, input))
                            .await;
                });
            }
        });
        Ok(Self {
            endpoint,
            task,
            path,
        })
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.path);
    }
}
async fn frame(stream: &mut TcpStream) -> Result<Value> {
    let mut bytes = Vec::new();
    BufReader::new(stream.take(8193))
        .read_until(b'\n', &mut bytes)
        .await?;
    ensure!(
        bytes.len() <= 8192 && bytes.last() == Some(&b'\n'),
        "Invalid control frame"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
async fn handle(
    mut stream: TcpStream,
    auth: &str,
    input: mpsc::Sender<crate::Input>,
) -> Result<()> {
    let request = frame(&mut stream).await?;
    ensure!(
        request["token"].as_str() == Some(auth),
        "Invalid session token"
    );
    let action: Action = serde_json::from_value(request["action"].clone())?;
    let (reply, receive) = oneshot::channel();
    input
        .send(crate::Input::Control(Request { action, reply }))
        .await?;
    let result = receive
        .await
        .context("Accessor closed before applying the control")?;
    stream.write_all(format!("{result}\n").as_bytes()).await?;
    Ok(())
}
pub async fn call(endpoint: &Endpoint, action: Action) -> Result<Value> {
    ensure!(
        endpoint.address.ip().is_loopback(),
        "Control endpoint must be loopback"
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(endpoint.address)
            .await
            .context("The Accessor session is no longer running")?;
        stream
            .write_all(format!("{}\n", json!({"token":endpoint.token,"action":action})).as_bytes())
            .await?;
        frame(&mut stream).await
    })
    .await
    .context("Control outcome is unknown; check session status before retrying")?
}
pub fn from_environment() -> Result<Endpoint> {
    serde_json::from_str(&std::env::var("ACC_CONTROL_ENDPOINT").context("No live Accessor session is attached to this MCP connection; start this harness through Accessor")?).context("Invalid session endpoint")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn authenticated_control_waits_for_actual_application() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = Endpoint {
            address: listener.local_addr().unwrap(),
            token: "test-secret".into(),
        };
        let (input, mut rx) = mpsc::channel(1);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle(stream, "test-secret", input).await.unwrap();
        });
        let client = tokio::spawn(async move { call(&endpoint, Action::StopAlarm).await.unwrap() });
        let Some(crate::Input::Control(request)) = rx.recv().await else {
            panic!("missing control");
        };
        assert!(!client.is_finished());
        request.reply.send(json!({"stopped":true})).unwrap();
        assert_eq!(client.await.unwrap()["stopped"], true);
        server.await.unwrap();
    }
    #[tokio::test]
    async fn wrong_session_cannot_issue_control() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (input, mut rx) = mpsc::channel(1);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            assert!(handle(stream, "right", input).await.is_err());
        });
        assert!(call(
            &Endpoint {
                address,
                token: "wrong".into()
            },
            Action::Sleep
        )
        .await
        .is_err());
        server.await.unwrap();
        assert!(rx.try_recv().is_err());
    }
}

use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub async fn serve<F, Fut>(
    handler: F,
) -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(String, Value)>,
    tokio::task::JoinHandle<()>,
)
where
    F: Fn(String, Value, String) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = (u16, String)> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handler = std::sync::Arc::new(handler);
    let task = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            // A handler assertion that panics stops the server, so the test fails loudly.
            let (mut stream, _) = tokio::select! {
                accepted = listener.accept() => accepted.unwrap(),
                Some(Err(error)) = connections.join_next() => std::panic::resume_unwind(error.into_panic()),
            };
            let handler = handler.clone();
            let tx = tx.clone();
            connections.spawn(async move {
                let mut request = Vec::new();
                let (offset, length) = loop {
                    let mut bytes = [0; 4096];
                    let read = stream.read(&mut bytes).await.unwrap();
                    assert!(read > 0);
                    request.extend_from_slice(&bytes[..read]);
                    if let Some(offset) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..offset]).to_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length: "))
                            .unwrap_or("0")
                            .parse()
                            .unwrap();
                        if request.len() >= offset + 4 + length {
                            break (offset + 4, length);
                        }
                    }
                };
                let path = String::from_utf8_lossy(&request[..offset])
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                let value = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&request[offset..offset + length]).unwrap()
                };
                let (status, body) =
                    handler(path.clone(), value.clone(), String::from_utf8_lossy(&request[..offset]).into_owned()).await;
                let response = format!("HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nX-Request-Id: request-test-1\r\nRequest-Id: native-request-test-1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                stream.write_all(response.as_bytes()).await.unwrap();
                let _ = tx.send((path, value));
            });
        }
    });
    (base, rx, task)
}

use super::*;
use std::sync::{Arc, Mutex};
use tokio::net::UnixListener;

#[tokio::test]
async fn same_story_focus_retains_tab_and_changed_story_replaces_only_owned_tab() {
    let path = std::env::temp_dir().join(format!("hn-browser-{}.sock", std::process::id()));
    let listener = UnixListener::bind(&path).unwrap();
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let tabs = Arc::new(Mutex::new(vec![10_i64, 99]));
    let log = requests.clone();
    let browser_tabs = tabs.clone();
    let task = tokio::spawn(async move {
        let mut next = 11;
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            log.lock().unwrap().push(request.clone());
            let data = match request["cmd"].as_str().unwrap() {
                "targets" => {
                    json!({"tabs":browser_tabs.lock().unwrap().iter().map(|id|json!({"id":id})).collect::<Vec<_>>()})
                }
                "activate-tab" => {
                    assert!(browser_tabs
                        .lock()
                        .unwrap()
                        .contains(&request["tab"].as_i64().unwrap()));
                    json!({})
                }
                "where" => json!({}), // No native UI operation in this protocol-boundary test.
                "open-tab" => {
                    let id = next;
                    next += 1;
                    browser_tabs.lock().unwrap().push(id);
                    json!({"openedTab":id})
                }
                "close-tab" => {
                    browser_tabs
                        .lock()
                        .unwrap()
                        .retain(|id| Some(*id) != request["tab"].as_i64());
                    json!({})
                }
                _ => panic!("unexpected request"),
            };
            let response = json!({"id":request["id"],"ok":true,"data":data});
            reader
                .get_mut()
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
    });
    let target = Target {
        socket: path.to_str().unwrap().into(),
        tab: 10,
        story: 1,
        close_shortcut: true,
    };
    let (target, _) = open(crate::reader::tests::story(1), Some(target), true)
        .await
        .unwrap();
    assert_eq!(target.as_ref().unwrap().tab, 10);
    assert_eq!(*tabs.lock().unwrap(), vec![10, 99]);
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r["cmd"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["targets", "activate-tab", "where"]
    );
    requests.lock().unwrap().clear();
    let (target, _) = open(crate::reader::tests::story(1), target, false)
        .await
        .unwrap();
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "comments-only revisit must not navigate or focus"
    );
    requests.lock().unwrap().clear();
    let (target, _) = open(crate::reader::tests::story(2), target, false)
        .await
        .unwrap();
    assert_eq!(target.as_ref().unwrap().tab, 11);
    assert_eq!(*tabs.lock().unwrap(), vec![99, 11]);
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r["cmd"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["targets", "open-tab", "close-tab"]
    );
    // A user-closed owned tab must not cause an unrelated tab to be closed.
    tabs.lock().unwrap().retain(|id| *id != 11);
    requests.lock().unwrap().clear();
    let (target, _) = open(crate::reader::tests::story(2), target, false)
        .await
        .unwrap();
    assert_eq!(target.as_ref().unwrap().tab, 12);
    assert_eq!(*tabs.lock().unwrap(), vec![99, 12]);
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r["cmd"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["targets", "open-tab"]
    );
    task.abort();
    std::fs::remove_file(path).unwrap();
}

use filmcraft_shorts::{Planner, request_plan};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
};

fn serve(content: &str) -> (Planner, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = serde_json::to_vec(&json!({"choices":[{"message":{"content":content}}]})).unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
            if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let len = headers.lines().find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                if request.len() >= end + 4 + len {
                    break;
                }
            }
            assert!(request.len() < 1024 * 1024);
        }
        assert!(String::from_utf8_lossy(&request).contains("POST /v1/chat/completions"));
        let end = request.windows(4).position(|s| s == b"\r\n\r\n").unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&request[end + 4..]).unwrap();
        assert_eq!(payload["response_format"]["type"], "json_object");
        assert_eq!(payload["response_format"]["schema"]["properties"]["segments"]["maxItems"], 0);
        assert_eq!(payload["response_format"]["schema"]["additionalProperties"], false);
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
        stream.write_all(&body).unwrap();
    });
    (Planner { url: format!("http://{addr}/v1/chat/completions"), model: "local-test".into() }, handle)
}
#[test]
fn compatible_chat_completion_returns_structured_plan_and_rejects_invalid_json() {
    for (response, valid) in [(r#"{"title":"다시 시작","segments":[]}"#, true), ("not JSON", false)] {
        let (planner, handle) = serve(response);
        assert_eq!(request_plan(&planner, &json!({"brief":"다시 시작","groups":[]})).is_ok(), valid);
        handle.join().unwrap();
    }
}
#[test]
fn remote_and_disguised_remote_endpoints_are_refused() {
    for url in ["http://example.com/v1/chat/completions", "https://example.com/v1/chat/completions", "http://localhost:8000@example.com/v1/chat/completions"] {
        let planner = Planner { url: url.into(), model: "test".into() };
        assert!(request_plan(&planner, &json!({})).is_err());
    }
}

#[test]
fn http_errors_are_actionable_without_leaking_server_bodies() {
    for code in [400, 401, 503] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut b = [0; 8192];
            let _ = stream.read(&mut b);
            let body = "PRIVATE_BODY_MUST_NOT_LEAK";
            write!(stream, "HTTP/1.1 {code} Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let planner = Planner { url: format!("http://{addr}/v1/chat/completions"), model: "test".into() };
        let error = request_plan(&planner, &json!({"groups":[]})).unwrap_err();
        assert!(!error.contains("PRIVATE_BODY"));
        assert!(error.contains(if code == 401 { "접근" } else { "HTTP" }), "{error}");
        server.join().unwrap();
    }
}

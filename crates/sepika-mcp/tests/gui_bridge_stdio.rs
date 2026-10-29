#![cfg(all(windows, feature = "mcp"))]
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

#[test]
fn mcp_initialize_lists_only_gui_tool_and_relays_a_real_pipe_request() {
    let listener = sepika_ai_connection::listener::Listener::start().unwrap();
    let name = listener.name.clone();
    let worker = std::thread::spawn(move || {
        let event = listener
            .events
            .recv_timeout(std::time::Duration::from_secs(15))
            .unwrap();
        let sepika_ai_connection::listener::Event::Request(incoming) = event else {
            panic!()
        };
        assert_eq!(incoming.request.operation, "connect");
        assert_eq!(incoming.request.version, 1);
        incoming
            .response
            .send(sepika_ai_connection::protocol::Response::accepted(
                incoming.request.id,
                json!({"status":"awaiting_gui_approval"}),
            ))
            .unwrap();
        let _ = listener
            .events
            .recv_timeout(std::time::Duration::from_secs(15));
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_sepika-mcp"))
        .args(["--gui-pipe", &name])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let (lines_tx, lines_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in stdout.lines() {
            let Ok(line) = line else {
                break;
            };
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });
    fn send(stdin: &mut impl Write, value: Value) {
        writeln!(stdin, "{value}").unwrap();
        stdin.flush().unwrap();
    }
    fn receive(stdout: &std::sync::mpsc::Receiver<String>) -> Value {
        let line = stdout
            .recv_timeout(std::time::Duration::from_secs(15))
            .expect("MCP応答がありません");
        serde_json::from_str(&line).unwrap()
    }
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"deterministic-test","version":"1"}}}),
    );
    assert_eq!(
        receive(&lines_rx)["result"]["serverInfo"]["name"],
        "sepika-gui-bridge"
    );
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    );
    let tools = receive(&lines_rx);
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(tools["result"]["tools"][0]["name"], "gui_request");
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"gui_request","arguments":{"envelope":{"version":1,"id":"connection","operation":"connect"}}}}),
    );
    let result = receive(&lines_rx);
    let result: Value =
        serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(result["result"]["status"], "awaiting_gui_approval");
    drop(stdin);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("MCPサーバが終了しません");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    worker.join().unwrap();
}

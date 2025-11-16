use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Stdio};

use serde_json::{json, Value};

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

#[test]
fn mcp_server_search_and_show_skill() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let landing_skill = fx.skill_dir("landing-the-plane");
    fs::create_dir_all(&landing_skill).unwrap();
    fs::write(
        landing_skill.join("SKILL.md"),
        "---\nname: landing-the-plane\ndescription: landing checklist\n---\nAlways run the landing checklist before touching down.\n",
    )
    .unwrap();

    let mut child = fx
        .sk_process()
        .arg("mcp-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn mcp server");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    send_frame(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    let init = expect_response(&mut reader, 1);
    let instructions = init["result"]["instructions"].as_str().unwrap();
    assert!(
        instructions.contains("skills.search"),
        "instructions should mention skills.search: {instructions}"
    );

    send_frame(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    );

    send_frame(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    );
    let list_resp = expect_response(&mut reader, 2);
    let tools = list_resp["result"]["tools"].as_array().unwrap();
    assert!(
        tools
            .iter()
            .any(|tool| tool["name"].as_str() == Some("skills.show")),
        "tool list should include skills.show"
    );

    send_frame(
        &mut stdin,
        json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"tools/call",
            "params":{
                "name":"skills.search",
                "arguments":{"query":"landing","limit":5}
            }
        }),
    );
    let search_resp = expect_response(&mut reader, 3);
    let results = search_resp["result"]["structuredContent"]["results"]
        .as_array()
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0]["install_name"].as_str(),
        Some("landing-the-plane")
    );

    send_frame(
        &mut stdin,
        json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"tools/call",
            "params":{
                "name":"skills.show",
                "arguments":{"skillName":"landing-the-plane"}
            }
        }),
    );
    let show_resp = expect_response(&mut reader, 4);
    let body = show_resp["result"]["structuredContent"]["skill"]["body"]
        .as_str()
        .unwrap();
    assert!(body.contains("landing checklist"));

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
}

fn send_frame(stdin: &mut ChildStdin, payload: Value) {
    serde_json::to_writer(&mut *stdin, &payload).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
}

fn read_frame(reader: &mut BufReader<ChildStdout>) -> Value {
    let mut buf = String::new();
    let bytes = reader.read_line(&mut buf).expect("read line");
    assert!(bytes > 0, "mcp server closed pipe unexpectedly");
    serde_json::from_str(buf.trim_end()).expect("valid json line")
}

fn expect_response(reader: &mut BufReader<ChildStdout>, id: i64) -> Value {
    loop {
        let frame = read_frame(reader);
        if let Some(frame_id) = frame.get("id").and_then(|v| v.as_i64()) {
            if frame_id == id {
                return frame;
            }
        }
    }
}

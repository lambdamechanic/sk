use anyhow::Result;
use crossbeam_channel::Sender;
use notify::event::EventKind;
use serde_json::{json, Deserializer, Value};
use std::io::{BufReader, BufWriter, Write};
use std::thread;

pub(crate) fn spawn_reader_thread(tx: Sender<Value>) {
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let reader = BufReader::new(stdin);
        let stream = Deserializer::from_reader(reader).into_iter::<Value>();
        for frame in stream {
            match frame {
                Ok(value) => {
                    if tx.send(value).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    eprintln!("failed to decode MCP frame: {err}");
                    break;
                }
            }
        }
    });
}

pub(crate) fn relevant_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    )
}

pub(crate) fn send_response(
    id: Value,
    result: Value,
    writer: &mut BufWriter<std::io::StdoutLock<'_>>,
) -> Result<()> {
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    write_frame(writer, &response)
}

pub(crate) fn send_notification(
    writer: &mut BufWriter<std::io::StdoutLock<'_>>,
    method: &str,
    params: Value,
) -> Result<()> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    write_frame(writer, &payload)
}

pub(crate) fn send_error(
    id: Value,
    code: i32,
    message: &str,
    data: Option<Value>,
    writer: &mut BufWriter<std::io::StdoutLock<'_>>,
) -> Result<()> {
    let mut error = json!({
        "code": code,
        "message": message,
    });
    if let Some(data) = data {
        error["data"] = data;
    }
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error
    });
    write_frame(writer, &response)
}

fn write_frame(writer: &mut BufWriter<std::io::StdoutLock<'_>>, payload: &Value) -> Result<()> {
    let mut buffer = Vec::new();
    serde_json::to_writer(&mut buffer, payload)?;
    buffer.push(b'\n');
    writer.write_all(&buffer)?;
    Ok(())
}

//! Optional MCP stdio transport. It is only started when the app receives `--mcp`.
use std::{
    io::{BufRead, BufReader, Write},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use serde_json::{Value, json};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const REQUEST_MESSAGE: u32 = WM_APP + 8;

pub struct Request {
    pub message: Value,
    pub reply: Sender<Value>,
}

pub fn start(hwnd: isize) -> Receiver<Request> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("kgen-mcp-stdio".into())
        .spawn(move || serve(hwnd, tx))
        .expect("could not start MCP stdio thread");
    rx
}

fn serve(hwnd: isize, tx: Sender<Request>) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in BufReader::new(stdin.lock()).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                let response = json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("Parse error: {error}")}});
                if writeln!(stdout, "{response}")
                    .and_then(|_| stdout.flush())
                    .is_err()
                {
                    break;
                }
                continue;
            }
        };
        // Notifications have no id and do not receive a JSON-RPC response.
        if message.get("id").is_none() {
            continue;
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        if tx
            .send(Request {
                message,
                reply: reply_tx,
            })
            .is_err()
        {
            break;
        }
        let posted = unsafe {
            PostMessageW(
                Some(HWND(hwnd as *mut _)),
                REQUEST_MESSAGE,
                WPARAM(0),
                LPARAM(0),
            )
        };
        if posted.is_err() {
            break;
        }
        let Ok(response) = reply_rx.recv() else { break };
        if writeln!(stdout, "{response}")
            .and_then(|_| stdout.flush())
            .is_err()
        {
            break;
        }
    }
}

pub fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

pub fn error(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.into()}})
}

pub fn text_result(value: Value) -> Value {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    json!({"content":[{"type":"text","text":text}],"structuredContent":value,"isError":false})
}

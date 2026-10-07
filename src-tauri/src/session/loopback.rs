//! A local echo "shell" used to exercise the terminal pipeline before SSH lands.

use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthChar;

use super::SessionInput;

const BANNER: &str = "\x1b[1;36mZShell\x1b[0m loopback session — 输入会被回显，用于验证终端链路。\r\n\r\n";
const PROMPT: &str = "\x1b[32m$\x1b[0m ";

pub async fn run(output: Channel, mut input: mpsc::UnboundedReceiver<SessionInput>) {
    let send = |text: String| output.send(InvokeResponseBody::Raw(text.into_bytes())).is_ok();

    if !send(format!("{BANNER}{PROMPT}")) {
        return;
    }

    let mut line = String::new();
    while let Some(msg) = input.recv().await {
        let echoed = match msg {
            SessionInput::Data(data) => echo(&mut line, &String::from_utf8_lossy(&data)),
            SessionInput::Resize { cols, rows } => {
                format!("\x1b7\x1b[1;{}H\x1b[2m{cols}x{rows}\x1b[0m\x1b8", cols.saturating_sub(8).max(1))
            }
        };
        if !echoed.is_empty() && !send(echoed) {
            break;
        }
    }
}

fn echo(line: &mut String, data: &str) -> String {
    // Escape sequences (arrow keys etc.) would move the cursor around; ignore them.
    if data.starts_with('\x1b') {
        return String::new();
    }

    let mut out = String::new();
    for c in data.chars() {
        match c {
            '\r' => {
                out.push_str("\r\n");
                if !line.is_empty() {
                    out.push_str(&format!("你输入了: {line}\r\n"));
                    line.clear();
                }
                out.push_str(PROMPT);
            }
            '\x7f' | '\x08' => {
                if let Some(removed) = line.pop() {
                    out.push_str(&"\x08 \x08".repeat(removed.width().unwrap_or(1)));
                }
            }
            '\x03' => {
                line.clear();
                out.push_str("^C\r\n");
                out.push_str(PROMPT);
            }
            c if !c.is_control() => {
                line.push(c);
                out.push(c);
            }
            _ => {}
        }
    }
    out
}

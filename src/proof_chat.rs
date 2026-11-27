//! A loopback Chat Completions fixture for scripted UI verification.
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::io_error;

type Server = thread::JoinHandle<Result<(), String>>;

pub(super) fn serve(
    turns: usize,
    mut respond: impl FnMut(usize, Value) -> Result<String, String> + Send + 'static,
) -> Result<(String, Server), String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(io_error)?;
    listener.set_nonblocking(true).map_err(io_error)?;
    let address = listener.local_addr().map_err(io_error)?;
    let worker = thread::spawn(move || {
        for turn in 0..turns {
            let start = Instant::now();
            let mut socket = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && start.elapsed() < Duration::from_secs(100) =>
                    {
                        thread::sleep(Duration::from_millis(15));
                    }
                    Err(error) => {
                        return Err(format!("Не получен запрос проверочного API. {error}"));
                    }
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(4)))
                .map_err(io_error)?;
            let mut request = BufReader::new(&socket);
            let mut header = String::new();
            request.read_line(&mut header).map_err(io_error)?;
            if !header.starts_with("POST /v1/chat/completions ") {
                return Err("Вызван неожиданный путь сервера.".into());
            }
            let mut length = 0;
            loop {
                header.clear();
                if request.read_line(&mut header).map_err(io_error)? == 0
                    || header.trim().is_empty()
                {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value
                        .trim()
                        .parse::<usize>()
                        .map_err(|error| error.to_string())?;
                }
            }
            if !(1..=65_536).contains(&length) {
                return Err("Тело запроса отсутствует или слишком велико.".into());
            }
            let mut body = vec![0; length];
            request.read_exact(&mut body).map_err(io_error)?;
            let wire = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
            let content = respond(turn, wire)?;
            let answer =
                json!({"choices":[{"message":{"content":content},"finish_reason":"stop"}]})
                    .to_string();
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len()).map_err(io_error)?;
        }
        Ok(())
    });
    Ok((format!("http://{address}/v1"), worker))
}

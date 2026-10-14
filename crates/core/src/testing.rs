//! For the tests only: a real HTTP server on a loopback port that answers as the test tells it to and
//! remembers what it was asked, and a token store in memory.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::platform::{PlatformError, Result, TokenStore};

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// With the query string, as sent.
    pub path: String,
    /// Names in lower case.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn form(&self) -> HashMap<String, String> {
        url::form_urlencoded::parse(&self.body)
            .into_owned()
            .collect()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

pub struct Reply {
    status: u16,
    body: String,
}

impl Reply {
    pub fn status(status: u16) -> Self {
        Self {
            status,
            body: String::new(),
        }
    }

    pub fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

pub struct FakeServer {
    port: u16,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
}

impl FakeServer {
    pub fn start(handler: impl Fn(&Request) -> Reply + Send + 'static) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let (seen, stopped) = (requests.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Some(request) = read(&mut stream) {
                            seen.lock().unwrap().push(request.clone());
                            let reply = handler(&request);
                            let _ = write!(
                                stream,
                                "HTTP/1.1 {} X\r\nContent-Type: application/json\r\n\
                                 Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                                reply.status,
                                reply.body.len(),
                                reply.body
                            );
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            port,
            requests,
            stop,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn read(stream: &mut TcpStream) -> Option<Request> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
        }
    };

    let head = String::from_utf8(bytes[..head_end].to_vec()).ok()?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let (method, path) = (request_line.next()?, request_line.next()?);
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();

    let length = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0usize);
    let mut body = bytes[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(read) => body.extend_from_slice(&chunk[..read]),
        }
    }

    Some(Request {
        method: method.into(),
        path: path.into(),
        headers,
        body,
    })
}

/// A token store in memory that also counts what was written to it, in order.
#[derive(Default)]
pub struct MemoryStore {
    token: Mutex<Option<String>>,
    pub saved: Mutex<Vec<String>>,
    /// While set, saving fails and changes nothing.
    pub fail_saves: AtomicBool,
}

impl MemoryStore {
    pub fn holding(token: &str) -> Self {
        Self {
            token: Mutex::new(Some(token.into())),
            ..Self::default()
        }
    }

    pub fn current(&self) -> Option<String> {
        self.token.lock().unwrap().clone()
    }
}

impl TokenStore for MemoryStore {
    fn load(&self) -> Result<Option<String>> {
        Ok(self.current())
    }

    fn save(&self, refresh_token: &str) -> Result<()> {
        if self.fail_saves.load(Ordering::SeqCst) {
            return Err(PlatformError::Os {
                operation: "saving",
                detail: "the test said so".into(),
            });
        }
        *self.token.lock().unwrap() = Some(refresh_token.into());
        self.saved.lock().unwrap().push(refresh_token.into());
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        *self.token.lock().unwrap() = None;
        Ok(())
    }
}

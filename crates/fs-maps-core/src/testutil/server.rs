//! Loopback HTTP server with `Range` support.
//!
//! Pattern: Fake (test double) — a minimal in-process server so tests and the example exercise
//! real HTTP range requests without touching any public server.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serves one file at `/map.pmtiles` with `Range` support (206 only).
pub struct RangeServer {
    /// URL of the served file.
    pub url: String,
    /// Number of requests received.
    pub requests: Arc<AtomicUsize>,
    /// `User-Agent` of every request.
    pub user_agents: Arc<Mutex<Vec<String>>>,
    script: Arc<Mutex<Script>>,
    task: tokio::task::JoinHandle<()>,
}

/// What the server answers with now: tests change it while it runs.
struct Script {
    data: Arc<Vec<u8>>,
    etag: String,
    redirect: Option<String>,
    padding: usize,
}

impl Drop for RangeServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl RangeServer {
    /// Serve `file` on a random loopback port.
    pub async fn start(file: &Path) -> std::io::Result<Self> {
        let script = Arc::new(Mutex::new(Script {
            data: Arc::new(std::fs::read(file)?),
            etag: "\"fixture\"".into(),
            redirect: None,
            padding: 0,
        }));
        let shared = script.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/map.pmtiles", listener.local_addr()?);
        let requests = Arc::new(AtomicUsize::new(0));
        let user_agents = Arc::new(Mutex::new(Vec::new()));
        let (rq, ua) = (requests.clone(), user_agents.clone());
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let (script, rq, ua) = (shared.clone(), rq.clone(), ua.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let Ok(n) = sock.read(&mut buf).await else {
                        return;
                    };
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    rq.fetch_add(1, Ordering::SeqCst);
                    let header = |name: &str| {
                        req.lines().find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
                        })
                    };
                    ua.lock()
                        .unwrap()
                        .push(header("user-agent").unwrap_or_default());
                    let range = header("range").and_then(|r| {
                        let r = r.strip_prefix("bytes=")?;
                        let (a, b) = r.split_once('-')?;
                        Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                    });
                    let path = req.split_whitespace().nth(1).unwrap_or("").to_owned();
                    let (data, etag, redirect, padding) = {
                        let s = script.lock().unwrap();
                        (
                            s.data.clone(),
                            s.etag.clone(),
                            s.redirect.clone(),
                            s.padding,
                        )
                    };
                    let redirect = redirect.filter(|to| !to.ends_with(&path));
                    let resp = match range {
                        _ if redirect.is_some() => format!(
                            "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            redirect.unwrap_or_default()
                        )
                        .into_bytes(),
                        Some((a, b)) if a < data.len() => {
                            let b = b.min(data.len() - 1);
                            let body = &data[a..=b];
                            let mut r = format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{}\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
                                body.len() + padding,
                                data.len()
                            )
                            .into_bytes();
                            r.extend_from_slice(body);
                            r.resize(r.len() + padding, 0);
                            r
                        }
                        _ => b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                    };
                    let _ = sock.write_all(&resp).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Ok(Self {
            url,
            requests,
            user_agents,
            script,
            task,
        })
    }

    /// Serve another file from now on, under another `ETag`.
    pub fn replace_archive(&self, file: &Path, etag: &str) -> std::io::Result<()> {
        let data = Arc::new(std::fs::read(file)?);
        let mut script = self.script.lock().unwrap();
        script.data = data;
        script.etag = format!("\"{etag}\"");
        Ok(())
    }

    /// Answer every request for another path with a redirect to `url`.
    pub fn redirect_to(&self, url: &str) {
        self.script.lock().unwrap().redirect = Some(url.to_owned());
    }

    /// Send `bytes` more than was asked for, with every answer.
    pub fn pad_answers(&self, bytes: usize) {
        self.script.lock().unwrap().padding = bytes;
    }
}

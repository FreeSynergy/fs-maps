//! Test/example helpers: a loopback HTTP server with `Range` support and a PMTiles fixture.
//! Lets tests and the example exercise real HTTP range requests without touching any public
//! server.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::Result;
use crate::region::{RegionDownload, RegionSpec};
use crate::source::MemorySource;
use crate::tile::BBox;

/// Serves one file at `/map.pmtiles` with `Range` support (206 only).
pub struct RangeServer {
    pub url: String,
    pub requests: Arc<AtomicUsize>,
    pub user_agents: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for RangeServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl RangeServer {
    pub async fn start(file: &Path) -> std::io::Result<Self> {
        let data: Arc<Vec<u8>> = Arc::new(std::fs::read(file)?);
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/map.pmtiles", listener.local_addr()?);
        let requests = Arc::new(AtomicUsize::new(0));
        let user_agents = Arc::new(Mutex::new(Vec::new()));
        let (rq, ua) = (requests.clone(), user_agents.clone());
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let (data, rq, ua) = (data.clone(), rq.clone(), ua.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let Ok(n) = sock.read(&mut buf).await else { return };
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    rq.fetch_add(1, Ordering::SeqCst);
                    let header = |name: &str| {
                        req.lines().find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
                        })
                    };
                    ua.lock().unwrap().push(header("user-agent").unwrap_or_default());
                    let range = header("range").and_then(|r| {
                        let r = r.strip_prefix("bytes=")?;
                        let (a, b) = r.split_once('-')?;
                        Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                    });
                    let resp = match range {
                        Some((a, b)) if a < data.len() => {
                            let b = b.min(data.len() - 1);
                            let body = &data[a..=b];
                            let mut r = format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{}\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n",
                                body.len(),
                                data.len()
                            )
                            .into_bytes();
                            r.extend_from_slice(body);
                            r
                        }
                        _ => b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                    };
                    let _ = sock.write_all(&resp).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Ok(Self { url, requests, user_agents, task })
    }
}

/// Writes a small PMTiles fixture (zoom 0..=`max_zoom`, every tile present, payload = its
/// coordinate plus filler) built with the same writer the region download uses.
pub async fn write_fixture(path: &Path, max_zoom: u8) -> Result<()> {
    let mut m = MemorySource::new("fixture");
    for z in 0..=max_zoom {
        for x in 0..(1u32 << z) {
            for y in 0..(1u32 << z) {
                m.insert(z, x, y, format!("MVT-FIXTURE {z}/{x}/{y} {}", "~".repeat(200)).into_bytes());
            }
        }
    }
    let spec = RegionSpec {
        name: "fixture".into(),
        bbox: BBox::new(-180.0, -85.0, 180.0, 85.0)?,
        min_zoom: 0,
        max_zoom,
        attribution: None,
    };
    RegionDownload::new(Arc::new(m), spec)?.download(path, &mut |_| true).await?;
    Ok(())
}

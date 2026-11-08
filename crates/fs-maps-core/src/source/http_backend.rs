//! Range requests to a remote archive, within the usage policy and the limits.
//!
//! Pattern: Adapter over `reqwest` behind `AsyncBackend` — the backend of the `pmtiles` crate reads
//! whatever body a server sends. This one stops at what was asked for, follows a redirect only if the
//! policy allows it, and keeps the URL, whose query may hold a token, out of its errors and out of
//! every `Referer`.

use std::time::Duration;

use bytes::BytesMut;
use pmtiles::reqwest::header::{ETAG, HeaderValue, LAST_MODIFIED, RANGE};
use pmtiles::reqwest::{Client, Method, Request, StatusCode, Url, redirect};
use pmtiles::{AsyncBackend, BackendResponse, PmtError, PmtResult};

use super::policy::{check_user_agent, redirect_allowed};
use crate::error::Result;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: usize = 5;

/// The client of a remote source. A User-Agent no header can hold fails here, at the start.
pub(super) fn http_client(user_agent: &str) -> Result<Client> {
    check_user_agent(user_agent)?;
    let policy = redirect::Policy::custom(|attempt| {
        let allowed = attempt.previous().len() <= MAX_REDIRECTS
            && attempt
                .previous()
                .last()
                .is_some_and(|from| redirect_allowed(from, attempt.url()));
        if allowed {
            attempt.follow()
        } else {
            attempt.error("the usage policy refuses this redirect")
        }
    });
    let client = Client::builder()
        .user_agent(user_agent)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(policy)
        .referer(false)
        .build()
        .map_err(pmtiles::reqwest::Error::without_url)?;
    Ok(client)
}

fn clean(e: pmtiles::reqwest::Error) -> PmtError {
    PmtError::Http(e.without_url())
}

/// One remote archive.
pub(super) struct HttpBackend {
    pub(super) client: Client,
    pub(super) url: Url,
}

impl AsyncBackend for HttpBackend {
    async fn read(&self, offset: u64, length: usize) -> PmtResult<BackendResponse> {
        // A tile of no bytes asks for nothing; `offset + 0 - 1` would also wrap at offset 0.
        if length == 0 {
            return Ok(BackendResponse {
                bytes: bytes::Bytes::new(),
                data_version_string: None,
            });
        }
        let end = offset
            .checked_add(length as u64 - 1)
            .ok_or_else(|| PmtError::Reading(std::io::ErrorKind::InvalidInput.into()))?;
        let range = HeaderValue::try_from(format!("bytes={offset}-{end}"))?;
        let mut request = Request::new(Method::GET, self.url.clone());
        request.headers_mut().insert(RANGE, range);
        let response = self.client.execute(request).await.map_err(clean)?;
        // A directory pointed past the end: the archive got smaller since it was opened.
        if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
            return Err(PmtError::SourceModified);
        }
        let mut response = response.error_for_status().map_err(clean)?;
        if response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(PmtError::RangeRequestsUnsupported);
        }
        let headers = response.headers();
        let version = headers
            .get(ETAG)
            .or_else(|| headers.get(LAST_MODIFIED))
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut body = BytesMut::new();
        while let Some(chunk) = response.chunk().await.map_err(clean)? {
            if body.len() + chunk.len() > length {
                let got = body.len() + chunk.len();
                return Err(PmtError::UnexpectedNumberOfBytesReturned(length, got));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(BackendResponse {
            bytes: body.freeze(),
            data_version_string: version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{RangeServer, write_fixture};

    #[tokio::test]
    async fn a_read_of_nothing_or_past_the_end_of_numbers_does_not_panic() {
        let backend = HttpBackend {
            client: http_client("FreeSynergy-Maps-test/0.2").unwrap(),
            url: Url::parse("http://127.0.0.1:9/never-asked").unwrap(),
        };
        assert!(backend.read(0, 0).await.unwrap().bytes.is_empty());
        assert!(backend.read(u64::MAX, 2).await.is_err());
    }

    #[tokio::test]
    async fn a_body_longer_than_asked_is_not_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.pmtiles");
        write_fixture(&file, 1).await.unwrap();
        let server = RangeServer::start(&file).await.unwrap();
        let backend = HttpBackend {
            client: http_client("FreeSynergy-Maps-test/0.2").unwrap(),
            url: Url::parse(&server.url).unwrap(),
        };
        assert_eq!(backend.read(0, 100).await.unwrap().bytes.len(), 100);
        server.pad_answers(1);
        let e = backend.read(0, 100).await.unwrap_err();
        assert!(
            matches!(e, PmtError::UnexpectedNumberOfBytesReturned(100, 101)),
            "{e:?}"
        );
    }
}

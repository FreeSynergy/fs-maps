//! Range requests to a remote archive, within the usage policy and the limits.
//!
//! Pattern: Adapter over `reqwest` behind `AsyncBackend` — the backend of the `pmtiles` crate reads
//! whatever body a server sends. This one stops at what was asked for, follows a redirect only if the
//! policy allows it, and keeps the URL, which may hold credentials, out of its errors.

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
        let end = offset + length as u64 - 1;
        let range = HeaderValue::try_from(format!("bytes={offset}-{end}"))?;
        let mut request = Request::new(Method::GET, self.url.clone());
        request.headers_mut().insert(RANGE, range);
        let mut response = self
            .client
            .execute(request)
            .await
            .map_err(clean)?
            .error_for_status()
            .map_err(clean)?;
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

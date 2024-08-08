use std::{fmt::Debug, path::PathBuf};

use bytes::Bytes;
use futures::StreamExt;
use http::{
    header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT},
    HeaderMap, HeaderValue,
};
use reqwest::ClientBuilder;
use tokio::{fs::File, io::AsyncWriteExt};
use url::Url;

use crate::{
    api_error,
    error::ApiError,
    model::{
        AgentId, ApiToken, LogSegmentId, LogSegmentOperationResponse, LogSegmentRequest,
        LogSegmentUpdateRequest, ProcessId, ProcessStatus,
    },
};

static USER_AGENT_VALUE: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub struct Config {
    pub base_url: Url,
    pub api_token: ApiToken,
    pub temp_dir: PathBuf,
}

pub struct ApiClient {
    config: Config,
    client: reqwest::Client,
}

macro_rules! api_url {
    ($client:expr, $fmt:expr $(, $args:expr)*) => {
        $client.config.base_url.join(&format!($fmt, $($args),*)).expect("valid url")
    };
}

impl ApiClient {
    pub fn new(config: Config) -> Result<Self, ApiError> {
        let authorization_header = HeaderValue::try_from(config.api_token.clone())
            .map_err(|e| api_error!("Invalid api_key: {e}"))?;

        let mut default_headers = HeaderMap::new();
        default_headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        default_headers.insert(AUTHORIZATION, authorization_header);

        let client = ClientBuilder::new().default_headers(default_headers).build()?;

        Ok(ApiClient { config, client })
    }

    pub fn process_api(&self) -> ProcessApiClient {
        ProcessApiClient {
            config: &self.config,
            client: self,
        }
    }

    fn get(&self, url: Url) -> reqwest::RequestBuilder {
        self.client.get(url)
    }

    fn post_octet_stream(&self, url: Url, body: Bytes) -> reqwest::RequestBuilder {
        self.client
            .post(url)
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(body)
    }

    fn post_text(&self, url: Url, body: impl Into<String>) -> reqwest::RequestBuilder {
        self.client
            .post(url)
            .header(CONTENT_TYPE, "text/plain")
            .body(body.into())
    }

    fn post_json(&self, url: Url, body: &impl serde::Serialize) -> reqwest::RequestBuilder {
        self.client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .json(body)
    }
}

pub struct ProcessApiClient<'a> {
    config: &'a Config,
    client: &'a ApiClient,
}

impl Debug for ProcessApiClient<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessApiClient")
            .field("config.base_url", &self.config.base_url)
            .finish()
    }
}

impl<'a> ProcessApiClient<'a> {
    #[tracing::instrument]
    pub async fn update_status(
        &self,
        process_id: ProcessId,
        agent_id: AgentId,
        status: ProcessStatus,
    ) -> Result<(), ApiError> {
        let resp = self
            .client
            .post_text(
                api_url!(self, "/api/v1/process/{process_id}/status"),
                format!("{status}"),
            )
            .query(&[("agent_id", agent_id.0)])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(api_error!("Failed to update status: {}", resp.status()));
        }

        Ok(())
    }

    pub async fn download_state(&self, process_id: ProcessId) -> Result<PathBuf, ApiError> {
        let resp = self
            .client
            .get(api_url!(self, "/api/v1/process/{process_id}/state/snapshot"))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(api_error!("Failed to download process state: {}", resp.status()));
        }

        let path = self.new_state_file_path(process_id.to_string(), ".zip").await?;
        let mut file = File::create(&path)
            .await
            .map_err(|e| api_error!("Failed to create a temporary file: {}", e))?;

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk)
                .await
                .map_err(|e| api_error!("Failed to write to a temporary file: {}", e))?;
        }

        Ok(path)
    }

    async fn new_state_file_path<PREFIX: AsRef<str>, SUFFIX: AsRef<str>>(
        &self,
        prefix: PREFIX,
        suffix: SUFFIX,
    ) -> Result<PathBuf, ApiError> {
        let prefix = prefix.as_ref();
        let suffix = suffix.as_ref();
        let temp_dir = &self.config.temp_dir;

        let mut path = temp_dir.join(format!("{prefix}{suffix}"));
        let mut n = 0;
        loop {
            let exists = path.try_exists().map_err(|e| api_error!("IO error: {}", e))?;
            if !exists {
                break;
            } else {
                path = temp_dir.join(format!("{prefix}-{n}{suffix}"));
                n += 1;
            }
        }

        Ok(path)
    }

    pub async fn create_log_segment(
        &self,
        process_id: ProcessId,
        req: &LogSegmentRequest,
    ) -> Result<LogSegmentId, ApiError> {
        let resp = self
            .client
            .post_json(api_url!(self, "/api/v2/process/{process_id}/log/segment"), req)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(api_error!("Failed to update status: {}", resp.status()));
        }

        let resp = resp.json::<LogSegmentOperationResponse>().await?;
        Ok(resp.id)
    }

    pub async fn update_log_segment(
        &self,
        process_id: ProcessId,
        segment_id: LogSegmentId,
        req: &LogSegmentUpdateRequest,
    ) -> Result<(), ApiError> {
        let resp = self
            .client
            .post_json(
                api_url!(self, "/api/v2/process/{process_id}/log/segment/{segment_id}"),
                req,
            )
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(api_error!("Failed to update status: {}", resp.status()));
        }

        Ok(())
    }

    pub async fn append_to_log_segment(
        &self,
        process_id: ProcessId,
        segment_id: LogSegmentId,
        data: Bytes,
    ) -> Result<(), ApiError> {
        let resp = self
            .client
            .post_octet_stream(
                api_url!(self, "/api/v2/process/{process_id}/log/segment/{segment_id}/data"),
                data,
            )
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(api_error!("Failed to update status: {}", resp.status()));
        }

        Ok(())
    }
}

use std::path::PathBuf;

use http::{header, HeaderValue};
use url::Url;

use crate::{
    api_err, api_error,
    error::ApiError,
    model::{
        AgentId, ApiToken, LogSegmentId, LogSegmentOperationResponse, LogSegmentRequest,
        LogSegmentUpdateRequest, ProcessId, ProcessStatus, USER_AGENT_VALUE,
    },
};

macro_rules! get {
    ($client:expr, $url_fmt:expr $(, $args:expr)*) => {
        $client.parent.client
            .get($client.config.base_url.join(&format!($url_fmt, $($args),*)).expect("valid url"))
    };
}

macro_rules! post {
    ($client:expr, $url_fmt:expr $(, $args:expr)*) => {
        $client
            .parent
            .client
            .post($client.config.base_url.join(&format!($url_fmt, $($args),*)).expect("valid url"))
    };
}

macro_rules! post_json {
    ($client:expr, $body:expr, $url_fmt:expr $(, $args:expr)*) => {
        post!($client, $url_fmt $(, $args),*)
        .header(header::CONTENT_TYPE, "application/json")
        .json($body)
    };
}

pub struct Config {
    pub base_url: Url,
    pub api_token: ApiToken,
    pub temp_dir: PathBuf,
}

pub struct ApiClient {
    config: Config,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new(config: Config) -> Result<Self, ApiError> {
        let authorization_header =
            HeaderValue::try_from(&config.api_token).map_err(|e| api_error!("Invalid api_key: {e}"))?;

        let default_headers = {
            let mut m = header::HeaderMap::new();
            m.insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
            m.insert(header::AUTHORIZATION, authorization_header);
            m
        };

        let client = reqwest::ClientBuilder::new()
            .default_headers(default_headers)
            .build()?;

        Ok(ApiClient { config, client })
    }

    pub fn process_api(&self) -> ProcessApiClient {
        ProcessApiClient {
            config: &self.config,
            parent: self,
        }
    }
}

pub struct ProcessApiClient<'a> {
    config: &'a Config,
    parent: &'a ApiClient,
}

impl std::fmt::Debug for ProcessApiClient<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessApiClient")
            .field("config.base_url", &self.config.base_url)
            .finish()
    }
}

impl<'a> ProcessApiClient<'a> {
    pub async fn update_status(
        &self,
        process_id: ProcessId,
        agent_id: AgentId,
        status: ProcessStatus,
    ) -> Result<(), ApiError> {
        let req = format!("{status}");
        let resp = post!(self, "/api/v1/process/{process_id}/status")
            .query(&[("agent_id", agent_id.0)])
            .header(header::CONTENT_TYPE, "text/plain")
            .body(req)
            .send()
            .await?;

        if !resp.status().is_success() {
            return api_err!("Failed to update status: {}", resp.status());
        }

        Ok(())
    }

    pub async fn download_state(&self, process_id: ProcessId) -> Result<PathBuf, ApiError> {
        let resp = get!(self, "/api/v1/process/{process_id}/state/snapshot")
            .send()
            .await?;

        if !resp.status().is_success() {
            return api_err!("Failed to download process state: {}", resp.status());
        }

        let path = self.new_state_file_path(process_id.to_string(), ".zip").await?;
        let mut file = tokio::fs::File::create(&path)
            .await
            .map_err(|e| api_error!("Failed to create a temporary file: {}", e))?;

        use futures::StreamExt;
        use tokio::io::AsyncWriteExt;
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
        let resp = post_json!(self, req, "/api/v2/process/{process_id}/log/segment")
            .send()
            .await?;

        if !resp.status().is_success() {
            return api_err!("Failed to update status: {}", resp.status());
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
        let resp = post_json!(self, req, "/api/v2/process/{process_id}/log/segment/{segment_id}")
            .send()
            .await?;

        if !resp.status().is_success() {
            return api_err!("Failed to update status: {}", resp.status());
        }

        Ok(())
    }

    pub async fn append_to_log_segment(
        &self,
        process_id: ProcessId,
        segment_id: LogSegmentId,
        body: bytes::Bytes,
    ) -> Result<(), ApiError> {
        let resp = post!(self, "/api/v2/process/{process_id}/log/segment/{segment_id}/data")
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return api_err!("Failed to update status: {}", resp.status());
        }

        Ok(())
    }
}

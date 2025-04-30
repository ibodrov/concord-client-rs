use std::borrow::Cow;
use std::path::PathBuf;

use http::{
    HeaderValue,
    header::{self, HeaderMap},
};
use reqwest::multipart;
use url::Url;

use crate::model::{ApiToken, ProcessEntry, StartProcessResponse};
use crate::{
    api_err, api_error,
    error::ApiError,
    model::{
        AgentId, LogSegmentId, LogSegmentOperationResponse, LogSegmentRequest, LogSegmentUpdateRequest,
        ProcessId, ProcessStatus, SessionToken, USER_AGENT_VALUE,
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
    pub session_token: Option<SessionToken>,
    pub api_token: Option<ApiToken>,
    pub temp_dir: Option<PathBuf>,
}

pub struct ApiClient {
    config: Config,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new(config: Config) -> Result<Self, ApiError> {
        let mut default_headers = HeaderMap::new();
        default_headers.insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));

        if let Some(session_token) = &config.session_token {
            default_headers.insert(
                "X-Concord-SessionToken",
                HeaderValue::try_from(session_token).map_err(|e| api_error!("Invalid session_token: {e}"))?,
            );
        } else if let Some(api_token) = &config.api_token {
            default_headers.insert(
                "Authorization",
                HeaderValue::try_from(api_token).map_err(|e| api_error!("Invalid api_token: {e}"))?,
            );
        }

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

impl ProcessApiClient<'_> {
    pub async fn start_process<I, T>(&self, input: I) -> Result<StartProcessResponse, ApiError>
    where
        I: IntoIterator<Item = (T, multipart::Part)>,
        T: Into<Cow<'static, str>>,
    {
        let mut form = multipart::Form::new();
        for (k, v) in input.into_iter() {
            form = form.part(k, v);
        }

        let resp = post!(self, "/api/v1/process").multipart(form).send().await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            api_err!("Failed to start a process: {}", resp.status())
        }
    }

    pub async fn get_process(&self, process_id: &ProcessId) -> Result<ProcessEntry, ApiError> {
        let resp = get!(self, "/api/v1/process/{process_id}").send().await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            api_err!("Failed to get process: {}", resp.status())
        }
    }

    pub async fn update_status(
        &self,
        process_id: ProcessId,
        agent_id: AgentId,
        status: ProcessStatus,
    ) -> Result<(), ApiError> {
        let req = format!("{status}");
        let resp = post!(self, "/api/v1/process/{process_id}/status")
            .query(&[("agent_id", agent_id)])
            .header(header::CONTENT_TYPE, "text/plain")
            .body(req)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            api_err!("Failed to update status: {}", resp.status())
        }
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

        let temp_dir = self
            .config
            .temp_dir
            .clone()
            .ok_or_else(|| api_error!("config.temp_dir is not set"))?;

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

        if resp.status().is_success() {
            let resp = resp.json::<LogSegmentOperationResponse>().await?;
            Ok(resp.id)
        } else {
            api_err!("Failed to update status: {}", resp.status())
        }
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

        if resp.status().is_success() {
            Ok(())
        } else {
            api_err!("Failed to update status: {}", resp.status())
        }
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

        if resp.status().is_success() {
            Ok(())
        } else {
            api_err!("Failed to update status: {}", resp.status())
        }
    }
}

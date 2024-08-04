use std::fmt::Debug;

use http::{
    header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT},
    HeaderMap, HeaderValue,
};
use reqwest::ClientBuilder;
use url::Url;

use crate::{
    api_error,
    error::ApiError,
    model::{AgentId, ApiToken, ProcessId, ProcessStatus},
};

static USER_AGENT_VALUE: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub struct Config {
    pub base_url: Url,
    pub api_token: ApiToken,
}

pub struct ApiClient {
    base_url: Url,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new(Config { base_url, api_token }: Config) -> Result<Self, ApiError> {
        let authorization_header =
            HeaderValue::try_from(api_token).map_err(|e| api_error!("Invalid api_key: {e}"))?;

        let mut default_headers = HeaderMap::new();
        default_headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        default_headers.insert(AUTHORIZATION, authorization_header);

        let client = ClientBuilder::new().default_headers(default_headers).build()?;

        Ok(ApiClient { base_url, client })
    }

    pub fn process_api(&self) -> ProcessApiClient {
        ProcessApiClient {
            base_url: &self.base_url,
            client: &self.client,
        }
    }
}

pub struct ProcessApiClient<'a> {
    base_url: &'a Url,
    client: &'a reqwest::Client,
}

impl Debug for ProcessApiClient<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessApiClient")
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl<'a> ProcessApiClient<'a> {
    #[tracing::instrument]
    pub async fn update_status(
        &self,
        instance_id: ProcessId,
        agent_id: AgentId,
        status: ProcessStatus,
    ) -> Result<(), ApiError> {
        let url = self
            .base_url
            .join(&format!("/api/v1/process/{instance_id}/status"))?;

        let response = self
            .client
            .post(url)
            .query(&[("agent_id", agent_id.0)])
            .header(CONTENT_TYPE, "text/plain")
            .body(format!("{status}"))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(api_error!("Failed to update status: {}", response.status()));
        }

        Ok(())
    }
}

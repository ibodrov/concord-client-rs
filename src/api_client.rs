use std::fmt::Debug;

use reqwest::ClientBuilder;
use url::Url;

use crate::{
    api_error,
    error::ApiError,
    model::{AgentId, InstanceId, ProcessStatus},
};

pub struct Config {
    pub base_url: Url,
    pub auth_header: String,
}

pub struct ApiClient {
    base_url: Url,
    auth_header: String,
    client: reqwest::Client,
}

impl Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient")
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl ApiClient {
    pub fn new(config: Config) -> Result<Self, ApiError> {
        let client = ClientBuilder::new().build()?;

        let Config {
            base_url,
            auth_header,
        } = config;

        Ok(ApiClient {
            base_url,
            auth_header,
            client,
        })
    }

    #[tracing::instrument]
    pub async fn update_status(
        &self,
        instance_id: InstanceId,
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
            .header("Authorization", &self.auth_header)
            .header("Content-Type", "text/plain")
            .body(format!("{status}"))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(api_error!("Failed to update status: {}", response.status()));
        }

        Ok(())
    }
}

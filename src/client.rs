use std::time::Duration;

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::User;
use crate::error::Error;
use crate::instances::{
    CreateInstance, CreatedInstance, Instance, InstanceResponse, InstancesResponse,
};
use crate::offers::{Offer, OfferQuery, OffersResponse};

pub const DEFAULT_BASE_URL: &str = "https://console.vast.ai/api/v0";

/// Attempts at an idempotent request before its error is returned.
const READ_ATTEMPTS: u32 = 4;

/// A vast.ai API client. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    api_key: String,
}

impl std::fmt::Debug for Client {
    // Never print the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl Client {
    pub fn new(api_key: impl Into<String>) -> Result<Self, Error> {
        Self::with_base_url(api_key, DEFAULT_BASE_URL)
    }

    pub fn with_base_url(
        api_key: impl Into<String>,
        base: impl Into<String>,
    ) -> Result<Self, Error> {
        let api_key = api_key.into().trim().to_string();
        if api_key.is_empty() {
            return Err(Error::MissingApiKey);
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent(concat!("rs-vastai-sdk/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
            api_key,
        })
    }

    /// The account the key belongs to, and its credit.
    pub async fn current_user(&self) -> Result<User, Error> {
        self.read(Method::GET, "/users/current/", None).await
    }

    /// Search the market.
    pub async fn search_offers(&self, query: &OfferQuery) -> Result<Vec<Offer>, Error> {
        // A search changes nothing, so it is safe to retry although it is a POST.
        let r: OffersResponse = self
            .read(Method::POST, "/bundles/", Some(query.to_body()))
            .await?;
        Ok(r.offers)
    }

    /// Rent an offer. Not retried: a retry after a lost answer would rent twice.
    pub async fn create_instance(
        &self,
        offer_id: u64,
        spec: &CreateInstance,
    ) -> Result<CreatedInstance, Error> {
        let path = format!("/asks/{offer_id}/");
        let body = self.send(Method::PUT, &path, Some(spec.to_body())).await?;
        decode(&path, body)
    }

    /// Every instance on the account.
    pub async fn list_instances(&self) -> Result<Vec<Instance>, Error> {
        let r: InstancesResponse = self.read(Method::GET, "/instances/?owner=me", None).await?;
        Ok(r.instances)
    }

    /// One instance; `None` once it has been destroyed.
    pub async fn get_instance(&self, id: u64) -> Result<Option<Instance>, Error> {
        let path = format!("/instances/{id}/?owner=me");
        match self
            .read::<InstanceResponse>(Method::GET, &path, None)
            .await
        {
            Ok(r) => Ok(r.instances),
            Err(e) if e.status() == Some(404) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Destroy an instance: stop billing for it, storage included, and lose
    /// its disk. Destroying one already gone succeeds.
    pub async fn destroy_instance(&self, id: u64) -> Result<(), Error> {
        let path = format!("/instances/{id}/");
        match self.retrying(Method::DELETE, &path, None).await {
            Ok(_) => Ok(()),
            Err(e) if e.status() == Some(404) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Change an interruptible instance's bid, **per GPU**, $/h.
    pub async fn change_bid(&self, id: u64, price_per_gpu: f64) -> Result<(), Error> {
        let path = format!("/instances/bid_price/{id}/");
        let body = serde_json::json!({ "client_id": "me", "price": price_per_gpu });
        self.retrying(Method::PUT, &path, Some(body))
            .await
            .map(drop)
    }

    /// Stop (`false`) or start (`true`) an instance. A stopped instance keeps
    /// its disk and is billed for storage only.
    pub async fn set_running(&self, id: u64, running: bool) -> Result<(), Error> {
        let path = format!("/instances/{id}/");
        let state = if running { "running" } else { "stopped" };
        self.retrying(
            Method::PUT,
            &path,
            Some(serde_json::json!({ "state": state })),
        )
        .await
        .map(drop)
    }

    /// The container's log, the last `tail` lines of it. Vast prepares the log
    /// asynchronously and hands back a URL; this polls that URL until it is
    /// ready, for up to `wait`.
    pub async fn logs(&self, id: u64, tail: Option<u32>, wait: Duration) -> Result<String, Error> {
        let path = format!("/instances/request_logs/{id}/");
        let mut body = serde_json::json!({});
        if let Some(tail) = tail {
            body["tail"] = tail.to_string().into();
        }
        let answer = self.retrying(Method::PUT, &path, Some(body)).await?;
        let Some(url) = answer["result_url"].as_str() else {
            return Err(Error::Refused {
                method: "PUT",
                path,
                message: answer.to_string(),
            });
        };
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let response = self.http.get(url).send().await?;
            if response.status().is_success() {
                return Ok(response.text().await?);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Status {
                    method: "GET",
                    path: "logs result_url".into(),
                    status: response.status().as_u16(),
                    body: "the log was not ready in time".into(),
                });
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn read<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, Error> {
        let v = self.retrying(method, path, body).await?;
        decode(path, v)
    }

    /// For requests that are safe to repeat. Backs off on 429 and 5xx.
    async fn retrying(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, Error> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.send(method.clone(), path, body.clone()).await {
                Err(e) if e.is_transient() && attempt < READ_ATTEMPTS => {
                    let wait = Duration::from_millis(500 * 2u64.pow(attempt - 1));
                    tracing::debug!(error = %e, attempt, wait_ms = wait.as_millis() as u64, "vast.ai call failed; retrying");
                    tokio::time::sleep(wait).await;
                }
                other => return other,
            }
        }
    }

    async fn send(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, Error> {
        let method_name = method_name(&method);
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.api_key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(Error::Status {
                method: method_name,
                path: path.to_string(),
                status: status.as_u16(),
                body: text,
            });
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        let value: Value = serde_json::from_str(&text).map_err(|source| Error::Decode {
            path: path.to_string(),
            source,
        })?;
        if value.get("success") == Some(&Value::Bool(false)) {
            let message = value
                .get("msg")
                .or_else(|| value.get("error"))
                .map(|m| {
                    m.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| m.to_string())
                })
                .unwrap_or_else(|| value.to_string());
            return Err(Error::Refused {
                method: method_name,
                path: path.to_string(),
                message,
            });
        }
        Ok(value)
    }
}

fn decode<T: DeserializeOwned>(path: &str, v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(|source| Error::Decode {
        path: path.to_string(),
        source,
    })
}

fn method_name(m: &Method) -> &'static str {
    match *m {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::DELETE => "DELETE",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_key_is_refused() {
        assert!(matches!(Client::new("  "), Err(Error::MissingApiKey)));
    }

    #[test]
    fn debug_never_prints_the_key() {
        let c = Client::new("secret-key-value").unwrap();
        assert!(!format!("{c:?}").contains("secret-key-value"));
    }

    #[test]
    fn only_rate_limits_and_server_errors_are_transient() {
        let status = |s| Error::Status {
            method: "GET",
            path: "/".into(),
            status: s,
            body: String::new(),
        };
        assert!(status(429).is_transient());
        assert!(status(503).is_transient());
        assert!(!status(400).is_transient());
        assert!(!status(404).is_transient());
    }
}

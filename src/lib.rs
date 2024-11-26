use alloy_primitives::hex::encode_prefixed;
pub use alloy_primitives::U256;
use alloy_signer_local::PrivateKeySigner;
pub use anyhow::{Context, Result as ClientResult};
use config::get_contract_config;
use reqwest::header::HeaderName;
use reqwest::Client;
use reqwest::Method;
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

mod config;
mod eth_utils;
mod headers;
mod utils;

pub use eth_utils::EthSigner;
use headers::{create_l1_headers, create_l2_headers};

#[derive(Debug, Deserialize)]
struct ApiKeysResponse {
    #[serde(rename = "apiKeys")]
    api_keys: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct MidpointResponse {
    pub mid: String,
}

#[derive(Debug, Deserialize)]
pub struct PriceResponse {
    pub price: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Side {
    BUY,
    SELL,
}

impl Side {
    fn as_str(&self) -> &'static str {
        match self {
            Side::BUY => "BUY",
            Side::SELL => "SELL",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BookParams {
    pub token_id: String,
    pub side: Side,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct ApiCreds {
    #[serde(rename = "apiKey")]
    api_key: String,
    secret: String,
    passphrase: String,
}

#[derive(Default)]
pub struct ClobClient {
    host: String,
    http_client: Client,
    signer: Option<Box<dyn EthSigner>>,
    chain_id: Option<u64>,
    api_creds: Option<ApiCreds>,
}

impl ClobClient {
    // TODO: initial headers, gzip
    pub fn new(host: &str) -> Self {
        Self {
            host: host.to_owned(),
            http_client: Client::new(),
            ..Default::default()
        }
    }
    pub fn with_l1_headers(host: &str, key: &str, chain_id: u64) -> Self {
        Self {
            host: host.to_owned(),
            http_client: Client::new(),
            signer: Some(Box::new(
                key.parse::<PrivateKeySigner>()
                    .expect("Invalid private key"),
            )),
            chain_id: Some(chain_id),
            api_creds: None,
        }
    }

    pub fn with_l2_headers(host: &str, key: &str, chain_id: u64, api_creds: ApiCreds) -> Self {
        Self {
            host: host.to_owned(),
            http_client: Client::new(),
            signer: Some(Box::new(
                key.parse::<PrivateKeySigner>()
                    .expect("Invalid private key"),
            )),
            chain_id: Some(chain_id),
            api_creds: Some(api_creds),
        }
    }
    pub fn set_api_creds(&mut self, api_creds: ApiCreds) {
        self.api_creds = Some(api_creds);
    }

    #[inline]
    fn get_l1_parameters(&self) -> (&impl EthSigner, u64) {
        let signer = self.signer.as_ref().expect("Signer is not set");
        let chain_id = self.chain_id.expect("Chain id is not set");
        (signer, chain_id)
    }

    #[inline]
    fn get_l2_parameters(&self) -> (&impl EthSigner, &ApiCreds) {
        let signer = self.signer.as_ref().expect("Signer is not set");
        (
            signer,
            self.api_creds.as_ref().expect("API credentials not set."),
        )
    }

    pub fn get_address(&self) -> Option<String> {
        Some(encode_prefixed(self.signer.as_ref()?.address().as_slice()))
    }

    pub fn get_collateral_address(&self) -> Option<String> {
        Some(get_contract_config(self.chain_id?, false)?.collateral)
    }

    pub fn get_conditional_address(&self) -> Option<String> {
        Some(get_contract_config(self.chain_id?, false)?.conditional_tokens)
    }

    pub fn get_exchange_address(&self) -> Option<String> {
        Some(get_contract_config(self.chain_id?, false)?.exchange)
    }

    fn create_request_with_headers(
        &self,
        method: Method,
        endpoint: &str,
        headers: impl Iterator<Item = (&'static str, String)>,
    ) -> RequestBuilder {
        let req = self
            .http_client
            .request(method, format!("{}{endpoint}", &self.host));

        headers.fold(req, |r, (k, v)| r.header(HeaderName::from_static(k), v))
    }

    pub async fn get_ok(&self) -> bool {
        self.http_client
            .get(format!("{}/", &self.host))
            .send()
            .await
            .is_ok()
    }

    pub async fn get_server_time(&self) -> ClientResult<u64> {
        let resp = self
            .http_client
            .get(format!("{}/time", &self.host))
            .send()
            .await?
            .text()
            .await?
            .parse::<u64>()?;
        Ok(resp)
    }

    pub async fn create_api_key(&self, nonce: Option<U256>) -> ClientResult<ApiCreds> {
        let method = Method::POST;
        let endpoint = "/auth/api-key";
        let (signer, _) = self.get_l1_parameters();
        let headers = create_l1_headers(signer, nonce)?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.send().await?.json::<ApiCreds>().await?)
    }

    pub async fn derive_api_key(&self, nonce: Option<U256>) -> ClientResult<ApiCreds> {
        let method = Method::GET;
        let endpoint = "/auth/derive-api-key";
        let (signer, _) = self.get_l1_parameters();
        let headers = create_l1_headers(signer, nonce)?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.send().await?.json::<ApiCreds>().await?)
    }

    pub async fn create_or_derive_api_key(&self, nonce: Option<U256>) -> ClientResult<ApiCreds> {
        let creds = self.create_api_key(nonce).await;
        if creds.is_err() {
            return self.derive_api_key(nonce).await;
        }
        creds
    }

    pub async fn get_api_keys(&self) -> ClientResult<Vec<String>> {
        let method = Method::GET;
        let endpoint = "/auth/api-keys";
        let (signer, creds) = self.get_l2_parameters();
        let headers = create_l2_headers(signer, creds, method.as_str(), endpoint, None)?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.send().await?.json::<ApiKeysResponse>().await?.api_keys)
    }

    pub async fn delete_api_key(&self) -> ClientResult<String> {
        let method = Method::DELETE;
        let endpoint = "/auth/api-key";
        let (signer, creds) = self.get_l2_parameters();
        let headers = create_l2_headers(signer, creds, method.as_str(), endpoint, None)?;
        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.send().await?.text().await?)
    }

    pub async fn get_midpoint(&self, token_id: &str) -> ClientResult<MidpointResponse> {
        Ok(self
            .http_client
            .get(format!("{}/midpoint", &self.host))
            .query(&[("token_id", token_id)])
            .send()
            .await?
            .json::<MidpointResponse>()
            .await?)
    }

    pub async fn get_midpoints(
        &self,
        token_ids: &[String],
    ) -> ClientResult<HashMap<String, String>> {
        let v = token_ids
            .iter()
            .map(|b| HashMap::from([("token_id", b.clone())]))
            .collect::<Vec<HashMap<&str, String>>>();

        Ok(self
            .http_client
            .post(format!("{}/midpoints", &self.host))
            .json(&v)
            .send()
            .await?
            .json::<HashMap<String, String>>()
            .await?)
    }

    pub async fn get_price(&self, token_id: &str, side: Side) -> ClientResult<PriceResponse> {
        Ok(self
            .http_client
            .get(format!("{}/price", &self.host))
            .query(&[("token_id", token_id)])
            .query(&[("side", side.as_str())])
            .send()
            .await?
            .json::<PriceResponse>()
            .await?)
    }

    pub async fn get_prices(
        &self,
        book_params: &[BookParams],
    ) -> ClientResult<HashMap<String, HashMap<Side, String>>> {
        let v = book_params
            .iter()
            .map(|b| {
                HashMap::from([
                    ("token_id", b.token_id.clone()),
                    ("side", b.side.as_str().to_owned()),
                ])
            })
            .collect::<Vec<HashMap<&str, String>>>();

        Ok(self
            .http_client
            .post(format!("{}/prices", &self.host))
            .json(&v)
            .send()
            .await?
            .json::<HashMap<String, HashMap<Side, String>>>()
            .await?)
    }
}

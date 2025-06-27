use alloy_primitives::hex::encode_prefixed;
pub use alloy_primitives::U256;
use alloy_signer_local::PrivateKeySigner;
pub use anyhow::{anyhow, Context, Result as ClientResult};
use config::get_contract_config;
use orders::OrderBuilder;
use orders::SignedOrderRequest;
use reqwest::header::HeaderName;
use reqwest::Client;
use reqwest::Method;
use reqwest::RequestBuilder;
use rust_decimal::Decimal;
pub use serde_json::Value;
use std::collections::HashMap;

// #[cfg(test)]
// mod tests;

mod config;
mod data;
mod eth_utils;
mod headers;
mod orders;
mod utils;

pub use data::*;
pub use eth_utils::EthSigner;
use headers::{create_l1_headers, create_l2_headers};

#[derive(Default)]
pub struct ClobClient {
    host: String,
    http_client: Client,
    signer: Option<Box<dyn EthSigner>>,
    chain_id: Option<u64>,
    api_creds: Option<ApiCreds>,
    order_builder: Option<OrderBuilder>,
}

const INITIAL_CURSOR: &str = "MA==";
const END_CURSOR: &str = "LTE=";

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
        let signer = Box::new(
            key.parse::<PrivateKeySigner>()
                .expect("Invalid private key"),
        );
        Self {
            host: host.to_owned(),
            http_client: Client::new(),
            signer: Some(signer.clone()),
            chain_id: Some(chain_id),
            api_creds: None,
            order_builder: Some(OrderBuilder::new(signer, None, None)),
        }
    }

    pub fn with_l2_headers(host: &str, key: &str, chain_id: u64, api_creds: ApiCreds) -> Self {
        let signer = Box::new(
            key.parse::<PrivateKeySigner>()
                .expect("Invalid private key"),
        );
        Self {
            host: host.to_owned(),
            http_client: Client::new(),
            signer: Some(signer.clone()),
            chain_id: Some(chain_id),
            api_creds: Some(api_creds),
            order_builder: Some(OrderBuilder::new(signer, None, None)),
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
        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.send().await?.json::<ApiKeysResponse>().await?.api_keys)
    }

    pub async fn delete_api_key(&self) -> ClientResult<String> {
        let method = Method::DELETE;
        let endpoint = "/auth/api-key";
        let (signer, creds) = self.get_l2_parameters();
        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;
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
    ) -> ClientResult<HashMap<String, Decimal>> {
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
            .json::<HashMap<String, Decimal>>()
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
    ) -> ClientResult<HashMap<String, HashMap<Side, Decimal>>> {
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
            .json::<HashMap<String, HashMap<Side, Decimal>>>()
            .await?)
    }

    pub async fn get_spread(&self, token_id: &str) -> ClientResult<SpreadResponse> {
        Ok(self
            .http_client
            .get(format!("{}/spread", &self.host))
            .query(&[("token_id", token_id)])
            .send()
            .await?
            .json::<SpreadResponse>()
            .await?)
    }

    pub async fn get_spreads(
        &self,
        token_ids: &[String],
    ) -> ClientResult<HashMap<String, Decimal>> {
        let v = token_ids
            .iter()
            .map(|b| HashMap::from([("token_id", b.clone())]))
            .collect::<Vec<HashMap<&str, String>>>();

        Ok(self
            .http_client
            .post(format!("{}/spreads", &self.host))
            .json(&v)
            .send()
            .await?
            .json::<HashMap<String, Decimal>>()
            .await?)
    }

    // cache
    pub async fn get_tick_size(&self, token_id: &str) -> ClientResult<Decimal> {
        Ok(self
            .http_client
            .get(format!("{}/tick-size", &self.host))
            .query(&[("token_id", token_id)])
            .send()
            .await?
            .json::<TickSizeResponse>()
            .await?
            .minimum_tick_size)
    }
    // Cache
    pub async fn get_neg_risk(&self, token_id: &str) -> ClientResult<bool> {
        Ok(self
            .http_client
            .get(format!("{}/neg-risk", &self.host))
            .query(&[("token_id", token_id)])
            .send()
            .await?
            .json::<NegRiskResponse>()
            .await?
            .neg_risk)
    }

    async fn resolve_tick_size(
        &self,
        token_id: &str,
        tick_size: Option<Decimal>,
    ) -> ClientResult<Decimal> {
        let min_tick_size = self
            .get_tick_size(token_id)
            .await
            .context("Error fetching tick size")?;

        match tick_size {
            None => Ok(min_tick_size),
            Some(t) => {
                if t < min_tick_size {
                    Err(anyhow!("Tick size {t} is smaller than min_tick_size {min_tick_size} for token_id: {token_id}"))
                } else {
                    Ok(t)
                }
            }
        }
    }

    async fn get_filled_order_options(
        &self,
        token_id: &str,
        options: Option<&CreateOrderOptions>,
    ) -> ClientResult<CreateOrderOptions> {
        let (tick_size, neg_risk) = match options {
            Some(o) => (o.tick_size, o.neg_risk),
            None => (None, None),
        };

        let tick_size = self.resolve_tick_size(token_id, tick_size).await?;

        let neg_risk = match neg_risk {
            Some(nr) => nr,
            None => self.get_neg_risk(token_id).await?,
        };

        Ok(CreateOrderOptions {
            neg_risk: Some(neg_risk),
            tick_size: Some(tick_size),
        })
    }

    fn is_price_in_range(&self, price: Decimal, tick_size: Decimal) -> bool {
        let min_price = tick_size;
        let max_price = Decimal::ONE - tick_size;

        if price < min_price || price > max_price {
            return false;
        }
        true
    }

    pub async fn create_order(
        &self,
        order_args: &OrderArgs,
        expiration: Option<u64>,
        extras: Option<ExtraOrderArgs>,
        options: Option<&CreateOrderOptions>,
    ) -> ClientResult<SignedOrderRequest> {
        let (_, chain_id) = self.get_l1_parameters();

        let create_order_options = self
            .get_filled_order_options(order_args.token_id.as_ref(), options)
            .await?;
        let expiration = expiration.unwrap_or(0);
        let extras = extras.unwrap_or_default();

        if !self.is_price_in_range(
            order_args.price,
            create_order_options.tick_size.expect("Should be filled"),
        ) {
            return Err(anyhow!("Price is not in range of tick_size"));
        }

        self.order_builder
            .as_ref()
            .expect("OrderBuilder not set")
            .create_order(
                chain_id,
                order_args,
                expiration,
                &extras,
                create_order_options,
            )
    }

    pub async fn get_order_book(&self, token_id: &str) -> ClientResult<OrderBookSummary> {
        Ok(self
            .http_client
            .get(format!("{}/book", &self.host))
            .query(&[("token_id", token_id)])
            .send()
            .await?
            .json::<OrderBookSummary>()
            .await?)
    }

    pub async fn get_order_books(
        &self,
        token_ids: &[String],
    ) -> ClientResult<Vec<OrderBookSummary>> {
        let v = token_ids
            .iter()
            .map(|b| HashMap::from([("token_id", b.clone())]))
            .collect::<Vec<HashMap<&str, String>>>();

        Ok(self
            .http_client
            .post(format!("{}/books", &self.host))
            .json(&v)
            .send()
            .await?
            .json::<Vec<OrderBookSummary>>()
            .await?)
    }

    async fn calculate_market_price(
        &self,
        token_id: &str,
        side: Side,
        amount: Decimal,
    ) -> ClientResult<Decimal> {
        let book = self.get_order_book(token_id).await?;
        let ob = self
            .order_builder
            .as_ref()
            .expect("No orderBuilder set for client!");
        match side {
            Side::BUY => ob.calculate_market_price(&book.asks, amount),
            Side::SELL => ob.calculate_market_price(&book.bids, amount),
        }
    }

    pub async fn create_market_order(
        &self,
        order_args: &MarketOrderArgs,
        extras: Option<ExtraOrderArgs>,
        options: Option<&CreateOrderOptions>,
    ) -> ClientResult<SignedOrderRequest> {
        let (_, chain_id) = self.get_l1_parameters();

        let create_order_options = self
            .get_filled_order_options(order_args.token_id.as_ref(), options)
            .await?;

        let extras = extras.unwrap_or_default();
        let price = self
            .calculate_market_price(&order_args.token_id, Side::BUY, order_args.amount)
            .await?;
        if !self.is_price_in_range(
            price,
            create_order_options.tick_size.expect("Should be filled"),
        ) {
            return Err(anyhow!("Price is not in range of tick_size"));
        }

        self.order_builder
            .as_ref()
            .expect("OrderBuilder not set")
            .create_market_order(chain_id, order_args, price, &extras, create_order_options)
    }

    pub async fn post_order(
        &self,
        order: SignedOrderRequest,
        order_type: OrderType,
    ) -> ClientResult<OrderResponse> {
        let (signer, creds) = self.get_l2_parameters();
        let body = PostOrder::new(order, creds.api_key.clone(), order_type);

        let method = Method::POST;
        let endpoint = "/order";

        let headers = create_l2_headers(signer, creds, method.as_str(), endpoint, Some(&body))?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.json(&body).send().await?.json::<OrderResponse>().await?)
    }

    pub async fn post_order_batch(
        &self,
        orders: Vec<(SignedOrderRequest, OrderType)>,
    ) -> ClientResult<BatchOrderResponse> {
        if orders.len() > 5 {
            return Err(anyhow!("Maximum of 5 orders allowed per batch"));
        }

        let (signer, creds) = self.get_l2_parameters();
        
        let post_orders: Vec<PostOrder> = orders
            .into_iter()
            .map(|(order, order_type)| PostOrder::new(order, creds.api_key.clone(), order_type))
            .collect();

        let body = PostOrderBatch::new(post_orders);

        let method = Method::POST;
        let endpoint = "/orders";

        let headers = create_l2_headers(signer, creds, method.as_str(), endpoint, Some(&body))?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.json(&body).send().await?.json::<BatchOrderResponse>().await?)
    }

    pub async fn create_and_post_order(&self, order_args: &OrderArgs) -> ClientResult<OrderResponse> {
        let order = self.create_order(order_args, None, None, None).await?;
        self.post_order(order, OrderType::GTC).await
    }

    pub async fn create_and_post_order_batch(
        &self,
        orders_args: &[(OrderArgs, OrderType)],
    ) -> ClientResult<BatchOrderResponse> {
        if orders_args.len() > 5 {
            return Err(anyhow!("Maximum of 5 orders allowed per batch"));
        }

        let mut signed_orders = Vec::new();
        
        for (order_args, order_type) in orders_args {
            let signed_order = self.create_order(order_args, None, None, None).await?;
            signed_orders.push((signed_order, *order_type));
        }

        self.post_order_batch(signed_orders).await
    }

    pub async fn cancel(&self, order_id: &str) -> ClientResult<Value> {
        let (signer, creds) = self.get_l2_parameters();
        let body = HashMap::from([("orderID", order_id)]);

        let method = Method::DELETE;
        let endpoint = "/order";

        let headers = create_l2_headers(signer, creds, method.as_str(), endpoint, Some(&body))?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.json(&body).send().await?.json::<Value>().await?)
    }

    pub async fn cancel_orders(&self, order_ids: &[String]) -> ClientResult<Value> {
        let (signer, creds) = self.get_l2_parameters();
        let method = Method::DELETE;
        let endpoint = "/orders";

        let headers = create_l2_headers(signer, creds, method.as_str(), endpoint, Some(order_ids))?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.json(order_ids).send().await?.json::<Value>().await?)
    }

    pub async fn cancel_all(&self) -> ClientResult<Value> {
        let (signer, creds) = self.get_l2_parameters();
        let method = Method::DELETE;
        let endpoint = "/cancel-all";

        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.send().await?.json::<Value>().await?)
    }

    pub async fn cancel_market_orders(
        &self,
        market: Option<&str>,
        asset_id: Option<&str>,
    ) -> ClientResult<Value> {
        let (signer, creds) = self.get_l2_parameters();
        let method = Method::DELETE;
        let endpoint = "/cancel-market-orders";
        let body = HashMap::from([
            ("market", market.unwrap_or("")),
            ("asset_id", asset_id.unwrap_or("")),
        ]);

        let headers = create_l2_headers(signer, creds, method.as_str(), endpoint, Some(&body))?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.json(&body).send().await?.json::<Value>().await?)
    }

    pub async fn get_orders(
        &self,
        params: Option<&OpenOrderParams>,
        next_cursor: Option<&str>,
    ) -> ClientResult<Vec<OpenOrder>> {
        let (signer, creds) = self.get_l2_parameters();
        let method = Method::GET;
        let endpoint = "/data/orders";
        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;

        let query_params = match params {
            None => Vec::new(),
            Some(p) => p.to_query_params(),
        };

        let mut next_cursor = next_cursor.unwrap_or(INITIAL_CURSOR).to_string();
        let mut output = Vec::new();
        while next_cursor != END_CURSOR {
            let req = self
                .http_client
                .request(method.clone(), format!("{}{endpoint}", &self.host))
                .query(&query_params)
                .query(&["next_cursor", &next_cursor]);

            let r = headers
                .clone()
                .into_iter()
                .fold(req, |r, (k, v)| r.header(HeaderName::from_static(k), v));

            let resp = r.send().await?.json::<Value>().await?;
            let new_cursor = resp["next_cursor"]
                .as_str()
                .expect("Failed to parse next cursor")
                .to_owned();

            next_cursor = new_cursor;

            let results = resp["data"].clone();
            let o = serde_json::from_value::<Vec<OpenOrder>>(results)
                .expect("Failed to parse data from order response");
            output.extend(o);
        }
        Ok(output)
    }

    pub async fn get_order(&self, order_id: &str) -> ClientResult<OpenOrder> {
        let (signer, creds) = self.get_l2_parameters();
        let method = Method::GET;
        let endpoint = &format!("/data/order/{order_id}");

        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req.send().await?.json::<OpenOrder>().await?)
    }

    pub async fn get_last_trade_price(&self, token_id: &str) -> ClientResult<Value> {
        Ok(self
            .http_client
            .get(format!("{}/last-trade-price", &self.host))
            .query(&[("token_id", token_id)])
            .send()
            .await?
            .json::<Value>()
            .await?)
    }

    pub async fn get_last_trade_prices(&self, token_ids: &[String]) -> ClientResult<Value> {
        let v = token_ids
            .iter()
            .map(|b| HashMap::from([("token_id", b.clone())]))
            .collect::<Vec<HashMap<&str, String>>>();

        Ok(self
            .http_client
            .post(format!("{}/last-trades-prices", &self.host))
            .json(&v)
            .send()
            .await?
            .json::<Value>()
            .await?)
    }

    pub async fn get_trades(
        &self,
        trade_params: Option<&TradeParams>,
        next_cursor: Option<&str>,
    ) -> ClientResult<Vec<Value>> {
        let (signer, creds) = self.get_l2_parameters();
        let method = Method::GET;
        let endpoint = "/data/trades";
        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;

        let query_params = match trade_params {
            None => Vec::new(),
            Some(p) => p.to_query_params(),
        };

        let mut next_cursor = next_cursor.unwrap_or(INITIAL_CURSOR).to_string();

        let mut output = Vec::new();
        while next_cursor != END_CURSOR {
            let req = self
                .http_client
                .request(method.clone(), format!("{}{endpoint}", &self.host))
                .query(&query_params)
                .query(&["next_cursor", &next_cursor]);

            let r = headers
                .clone()
                .into_iter()
                .fold(req, |r, (k, v)| r.header(HeaderName::from_static(k), v));

            let resp = r.send().await?.json::<Value>().await?;
            let new_cursor = resp["next_cursor"]
                .as_str()
                .expect("Failed to parse next cursor")
                .to_owned();

            next_cursor = new_cursor;

            let results = resp["data"].clone();
            output.push(results);
        }
        Ok(output)
    }

    pub async fn get_notifications(&self) -> ClientResult<Value> {
        let (signer, creds) = self.get_l2_parameters();

        let method = Method::GET;
        let endpoint = "/notifications";
        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req
            .query(&[(
                "signature_type",
                &self
                    .order_builder
                    .as_ref()
                    .expect("Orderbuilder not set")
                    .get_sig_type(),
            )])
            .send()
            .await?
            .json::<Value>()
            .await?)
    }

    pub async fn drop_notifications(&self, ids: &[String]) -> ClientResult<Value> {
        let (signer, creds) = self.get_l2_parameters();

        let method = Method::DELETE;
        let endpoint = "/notifications";
        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req
            .query(&[("ids", ids.join(","))])
            .send()
            .await?
            .json::<Value>()
            .await?)
    }

    pub async fn get_balance_allowance(
        &self,
        params: Option<BalanceAllowanceParams>,
    ) -> ClientResult<Value> {
        let mut params = params.unwrap_or_default();
        if params.signature_type.is_none() {
            params.set_signature_type(
                self.order_builder
                    .as_ref()
                    .expect("Orderbuilder not set")
                    .get_sig_type(),
            )
        }

        let query_params = params.to_query_params();

        let (signer, creds) = self.get_l2_parameters();

        let method = Method::GET;
        let endpoint = "/balance-allowance";
        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());
        Ok(req
            .query(&query_params)
            .send()
            .await?
            .json::<Value>()
            .await?)
    }

    pub async fn update_balance_allowance(
        &self,
        params: Option<BalanceAllowanceParams>,
    ) -> ClientResult<Value> {
        let mut params = params.unwrap_or_default();
        if params.signature_type.is_none() {
            params.set_signature_type(
                self.order_builder
                    .as_ref()
                    .expect("Orderbuilder not set")
                    .get_sig_type(),
            )
        }

        let query_params = params.to_query_params();

        let (signer, creds) = self.get_l2_parameters();

        let method = Method::GET;
        let endpoint = "/balance-allowance/update";
        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;

        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());
        Ok(req
            .query(&query_params)
            .send()
            .await?
            .json::<Value>()
            .await?)
    }

    pub async fn is_order_scoring(&self, order_id: &str) -> ClientResult<bool> {
        let (signer, creds) = self.get_l2_parameters();

        let method = Method::GET;
        let endpoint = "/order-scoring";
        let headers = create_l2_headers::<Value>(signer, creds, method.as_str(), endpoint, None)?;
        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req
            .query(&[("order_id", order_id)])
            .send()
            .await?
            .json::<Value>()
            .await?["scoring"]
            .as_bool()
            .expect("Unknown scoring value"))
    }

    pub async fn are_orders_scoring(
        &self,
        order_ids: &[&str],
    ) -> ClientResult<HashMap<String, bool>> {
        let (signer, creds) = self.get_l2_parameters();

        let method = Method::POST;
        let endpoint = "/orders-scoring";

        let headers = create_l2_headers(signer, creds, method.as_str(), endpoint, Some(order_ids))?;
        let req = self.create_request_with_headers(method, endpoint, headers.into_iter());

        Ok(req
            .json(order_ids)
            .send()
            .await?
            .json::<HashMap<String, bool>>()
            .await?)
    }

    pub async fn get_sampling_markets(&self, next_cursor: Option<&str>) -> ClientResult<MarketsResponse> {
        let next_cursor = next_cursor.unwrap_or(INITIAL_CURSOR);

        Ok(self
            .http_client
            .get(format!("{}/sampling-markets", &self.host))
            .query(&[("next_cursor", next_cursor)])
            .send()
            .await?
            .json::<MarketsResponse>()
            .await?)
    }

    pub async fn get_sampling_simplified_markets(
        &self,
        next_cursor: Option<&str>,
    ) -> ClientResult<SimplifiedMarketsResponse> {
        let next_cursor = next_cursor.unwrap_or(INITIAL_CURSOR);

        Ok(self
            .http_client
            .get(format!("{}/sampling-simplified-markets", &self.host))
            .query(&[("next_cursor", next_cursor)])
            .send()
            .await?
            .json::<SimplifiedMarketsResponse>()
            .await?)
    }

    pub async fn get_markets(&self, next_cursor: Option<&str>) -> ClientResult<MarketsResponse> {
        let next_cursor = next_cursor.unwrap_or(INITIAL_CURSOR);

        Ok(self
            .http_client
            .get(format!("{}/markets", &self.host))
            .query(&[("next_cursor", next_cursor)])
            .send()
            .await?
            .json::<MarketsResponse>()
            .await?)
    }

    pub async fn get_simplified_markets(
        &self,
        next_cursor: Option<&str>,
    ) -> ClientResult<SimplifiedMarketsResponse> {
        let next_cursor = next_cursor.unwrap_or(INITIAL_CURSOR);

        Ok(self
            .http_client
            .get(format!("{}/simplified-markets", &self.host))
            .query(&[("next_cursor", next_cursor)])
            .send()
            .await?
            .json::<SimplifiedMarketsResponse>()
            .await?)
    }

    pub async fn get_market(&self, condition_id: &str) -> ClientResult<Market> {
        Ok(self
            .http_client
            .get(format!("{}/markets/{condition_id}", &self.host))
            .send()
            .await?
            .json::<Market>()
            .await?)
    }

    pub async fn get_market_trades_events(&self, condition_id: &str) -> ClientResult<Value> {
        Ok(self
            .http_client
            .get(format!(
                "{}/live-activity/events/{condition_id}",
                &self.host
            ))
            .send()
            .await?
            .json::<Value>()
            .await?)
    }
}

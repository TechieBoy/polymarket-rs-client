use crate::Decimal;

use crate::SignedOrderRequest;
use alloy_primitives::U256;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fmt::Display;
use std::str::FromStr;

const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

pub enum AssetType {
    COLLATERAL,
    CONDITIONAL,
}

#[allow(clippy::to_string_trait_impl)]
impl ToString for AssetType {
    fn to_string(&self) -> String {
        match self {
            AssetType::COLLATERAL => "COLLATERAL".to_string(),
            AssetType::CONDITIONAL => "CONDITIONAL".to_string(),
        }
    }
}

#[derive(Default)]
pub struct BalanceAllowanceParams {
    pub asset_type: Option<AssetType>,
    pub token_id: Option<String>,
    pub signature_type: Option<u8>,
}

impl BalanceAllowanceParams {
    pub fn to_query_params(&self) -> Vec<(&str, String)> {
        let mut params = Vec::with_capacity(3);

        if let Some(x) = &self.asset_type {
            params.push(("asset_type", x.to_string()));
        }

        if let Some(x) = &self.token_id {
            params.push(("token_id", x.to_string()));
        }

        if let Some(x) = &self.signature_type {
            params.push(("signature_type", x.to_string()));
        }
        params
    }
}

impl BalanceAllowanceParams {
    pub fn set_signature_type(&mut self, s: u8) {
        self.signature_type = Some(s);
    }
}

#[derive(Debug)]
pub struct TradeParams {
    pub id: Option<String>,
    pub maker_address: Option<String>,
    pub market: Option<String>,
    pub asset_id: Option<String>,
    pub before: Option<u64>,
    pub after: Option<u64>,
}

impl TradeParams {
    pub fn to_query_params(&self) -> Vec<(&str, String)> {
        let mut params = Vec::with_capacity(4);

        if let Some(x) = &self.id {
            params.push(("id", x.clone()));
        }

        if let Some(x) = &self.asset_id {
            params.push(("asset_id", x.clone()));
        }

        if let Some(x) = &self.market {
            params.push(("market", x.clone()));
        }
        if let Some(x) = &self.before {
            params.push(("before", x.to_string()));
        }
        if let Some(x) = &self.after {
            params.push(("after", x.to_string()));
        }
        params
    }
}

#[derive(Debug, Deserialize)]
pub struct OpenOrder {
    pub associate_trades: Vec<String>,
    pub id: String,
    pub status: String,
    pub market: String,

    #[serde(with = "rust_decimal::serde::str")]
    pub original_size: Decimal,
    pub outcome: String,
    pub maker_address: String,
    pub owner: String,

    #[serde(with = "rust_decimal::serde::str")]
    pub price: Decimal,
    pub side: Side,

    #[serde(with = "rust_decimal::serde::str")]
    pub size_matched: Decimal,
    pub asset_id: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub expiration: u64,
    #[serde(rename = "type")]
    pub order_type: OrderType,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub created_at: u64,
}

#[derive(Debug)]
pub struct OpenOrderParams {
    pub id: Option<String>,
    pub asset_id: Option<String>,
    pub market: Option<String>,
}

impl OpenOrderParams {
    pub fn to_query_params(&self) -> Vec<(&str, &String)> {
        let mut params = Vec::with_capacity(4);

        if let Some(x) = &self.id {
            params.push(("id", x));
        }

        if let Some(x) = &self.asset_id {
            params.push(("asset_id", x));
        }

        if let Some(x) = &self.market {
            params.push(("market", x));
        }
        params
    }
}

fn deserialize_number_from_string<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr + serde::Deserialize<'de>,
    <T as FromStr>::Err: Display,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrInt<T> {
        String(String),
        Number(T),
    }

    match StringOrInt::<T>::deserialize(deserializer)? {
        StringOrInt::String(s) => s.parse::<T>().map_err(serde::de::Error::custom),
        StringOrInt::Number(i) => Ok(i),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostOrder {
    order: SignedOrderRequest,
    owner: String,
    order_type: OrderType,
}

impl PostOrder {
    pub fn new(order: SignedOrderRequest, owner: String, order_type: OrderType) -> Self {
        PostOrder {
            order,
            owner,
            order_type,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PostOrderBatch {
    orders: Vec<PostOrder>,
}

impl PostOrderBatch {
    pub fn new(orders: Vec<PostOrder>) -> Self {
        PostOrderBatch { orders }
    }
}

#[derive(Debug)]
pub struct OrderArgs {
    pub token_id: String,
    pub price: Decimal,
    pub size: Decimal,
    pub side: Side,
}

#[derive(Debug, Deserialize)]
pub struct OrderBookSummary {
    pub market: String,
    pub asset_id: String,
    pub hash: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub timestamp: u64,
    pub bids: Vec<OrderSummary>,
    pub asks: Vec<OrderSummary>,
}

#[derive(Debug)]
pub struct MarketOrderArgs {
    pub token_id: String,
    pub amount: Decimal,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OrderSummary {
    #[serde(with = "rust_decimal::serde::str")]
    pub price: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub size: Decimal,
}

impl OrderArgs {
    pub fn new(token_id: &str, price: Decimal, size: Decimal, side: Side) -> Self {
        OrderArgs {
            token_id: token_id.to_owned(),
            price,
            size,
            side,
        }
    }
}

#[derive(Debug)]
pub struct ExtraOrderArgs {
    pub fee_rate_bps: u32,
    pub nonce: U256,
    pub taker: String,
}

impl Default for ExtraOrderArgs {
    fn default() -> Self {
        ExtraOrderArgs {
            fee_rate_bps: 0,
            nonce: U256::ZERO,
            taker: ZERO_ADDRESS.into(),
        }
    }
}

#[derive(Debug, Default)]
pub struct CreateOrderOptions {
    pub tick_size: Option<Decimal>,
    pub neg_risk: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ApiKeysResponse {
    #[serde(rename = "apiKeys")]
    pub api_keys: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MidpointResponse {
    #[serde(with = "rust_decimal::serde::str")]
    pub mid: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct PriceResponse {
    #[serde(with = "rust_decimal::serde::str")]
    pub price: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct SpreadResponse {
    #[serde(with = "rust_decimal::serde::str")]
    pub spread: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct TickSizeResponse {
    pub minimum_tick_size: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct NegRiskResponse {
    pub neg_risk: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Hash, Eq, PartialEq)]
pub enum OrderType {
    GTC,
    FOK,
    GTD,
    FAK,
}

impl OrderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            OrderType::GTC => "GTC",
            OrderType::FOK => "FOK",
            OrderType::GTD => "GTD",
            OrderType::FAK => "FAK",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Side {
    BUY = 0,
    SELL = 1,
}

impl Side {
    pub fn as_str(&self) -> &'static str {
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
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MarketsResponse {
    pub limit: Decimal,
    pub count: Decimal,
    pub next_cursor: Option<String>,
    pub data: Vec<Market>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimplifiedMarketsResponse {
    pub limit: Decimal,
    pub count: Decimal,
    pub next_cursor: Option<String>,
    pub data: Vec<SimplifiedMarket>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Market {
    pub condition_id: String,
    pub tokens: [Token; 2],
    pub rewards: Rewards,
    pub min_incentive_size: Option<String>,
    pub max_incentive_spread: Option<String>,
    pub active: bool,
    pub closed: bool,

    pub question_id: String,
    pub minimum_order_size: Decimal,
    pub minimum_tick_size: Decimal,
    pub description: String,
    pub category: Option<String>,
    pub end_date_iso: Option<String>,
    pub game_start_time: Option<String>,
    pub question: String,
    pub market_slug: String,
    pub seconds_delay: Decimal,
    pub icon: String,
    pub fpmm: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimplifiedMarket {
    pub condition_id: String,
    pub tokens: [Token; 2],
    pub rewards: Rewards,
    pub min_incentive_size: Option<String>,
    pub max_incentive_spread: Option<String>,
    pub active: bool,
    pub closed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Token {
    pub token_id: String,
    pub outcome: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_msg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_hashes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

pub type BatchOrderResponse = Vec<OrderResponse>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Rewards {
    pub rates: Option<Value>,
    pub min_size: Decimal,
    pub max_spread: Decimal,
    pub event_start_date: Option<String>,
    pub event_end_date: Option<String>,
    pub in_game_multiplier: Option<Decimal>,
    pub reward_epoch: Option<Decimal>,
}

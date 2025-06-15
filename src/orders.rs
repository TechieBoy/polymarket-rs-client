use alloy_primitives::Address;
use alloy_primitives::U256;
use anyhow::anyhow;
use anyhow::{Context, Result};
use rand::thread_rng;
use rand::Rng;
use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy::{AwayFromZero, MidpointTowardZero, ToZero};

use serde::Serialize;

use crate::config::get_contract_config;
use crate::eth_utils::sign_order_message;
use crate::eth_utils::Order;
use crate::utils::get_current_unix_time_secs;
use crate::{
    CreateOrderOptions, EthSigner, ExtraOrderArgs, MarketOrderArgs, OrderArgs, OrderSummary, Side,
};

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::LazyLock;

/// Signature type for different wallet configurations
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SigType {
    /// ECDSA EIP712 signatures signed by EOAs (Externally Owned Accounts)
    Eoa = 0,
    /// EIP712 signatures signed by EOAs that own Polymarket Proxy wallets
    PolyProxy = 1,
    /// EIP712 signatures signed by EOAs that own Polymarket Gnosis safes
    PolyGnosisSafe = 2,
}

pub struct OrderBuilder {
    signer: Box<dyn EthSigner>,
    sig_type: SigType,
    funder: Address,
}

pub struct RoundConfig {
    price: u32,
    size: u32,
    amount: u32,
}

fn generate_seed() -> u64 {
    let mut rng = thread_rng();
    let y: f64 = rng.gen();
    let a: f64 = get_current_unix_time_secs() as f64 * y;
    a as u64
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedOrderRequest {
    pub salt: u64,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    pub token_id: String,
    pub maker_amount: String,
    pub taker_amount: String,
    pub expiration: String,
    pub nonce: String,
    pub fee_rate_bps: String,
    pub side: String,
    pub signature_type: u8,
    pub signature: String,
}

static ROUNDING_CONFIG: LazyLock<HashMap<Decimal, RoundConfig>> = LazyLock::new(|| {
    HashMap::from([
        (
            Decimal::from_str("0.1").unwrap(),
            RoundConfig {
                price: 1,
                size: 2,
                amount: 3,
            },
        ),
        (
            Decimal::from_str("0.01").unwrap(),
            RoundConfig {
                price: 2,
                size: 2,
                amount: 4,
            },
        ),
        (
            Decimal::from_str("0.001").unwrap(),
            RoundConfig {
                price: 3,
                size: 2,
                amount: 5,
            },
        ),
        (
            Decimal::from_str("0.0001").unwrap(),
            RoundConfig {
                price: 4,
                size: 2,
                amount: 6,
            },
        ),
    ])
});

fn decimal_to_token_u32(amt: Decimal) -> u32 {
    let mut amt = Decimal::from_scientific("1e6").expect("1e6 is not scientific") * amt;
    if amt.scale() > 0 {
        amt = amt.round_dp_with_strategy(0, MidpointTowardZero);
    }
    amt.try_into().expect("Couldn't round decimal to integer")
}

impl OrderBuilder {
    pub fn new(
        signer: Box<dyn EthSigner>,
        sig_type: Option<SigType>,
        funder: Option<Address>,
    ) -> Self {
        let sig_type = sig_type.unwrap_or(SigType::Eoa);
        let funder = funder.unwrap_or(signer.address());

        OrderBuilder {
            signer,
            sig_type,
            funder,
        }
    }

    pub fn get_sig_type(&self) -> u8 {
        self.sig_type as u8
    }

    fn fix_amount_rounding(&self, mut amt: Decimal, round_config: &RoundConfig) -> Decimal {
        if amt.scale() > round_config.amount {
            amt = amt.round_dp_with_strategy(round_config.amount + 4, AwayFromZero);
            if amt.scale() > round_config.amount {
                amt = amt.round_dp_with_strategy(round_config.amount, ToZero);
            }
        }
        amt
    }

    fn get_order_amounts(
        &self,
        side: Side,
        size: Decimal,
        price: Decimal,
        round_config: &RoundConfig,
    ) -> (u32, u32) {
        let raw_price = price.round_dp_with_strategy(round_config.price, MidpointTowardZero);

        match side {
            Side::BUY => {
                let raw_taker_amt = size.round_dp_with_strategy(round_config.size, ToZero);
                let raw_maker_amt = raw_taker_amt * raw_price;
                let raw_maker_amt = self.fix_amount_rounding(raw_maker_amt, round_config);
                (
                    decimal_to_token_u32(raw_maker_amt),
                    decimal_to_token_u32(raw_taker_amt),
                )
            }
            Side::SELL => {
                let raw_maker_amt = size.round_dp_with_strategy(round_config.size, ToZero);
                let raw_taker_amt = raw_maker_amt * raw_price;
                let raw_taker_amt = self.fix_amount_rounding(raw_taker_amt, round_config);

                (
                    decimal_to_token_u32(raw_maker_amt),
                    decimal_to_token_u32(raw_taker_amt),
                )
            }
        }
    }

    fn get_market_order_amounts(
        &self,
        amount: Decimal,
        price: Decimal,
        round_config: &RoundConfig,
    ) -> (u32, u32) {
        let raw_maker_amt = amount.round_dp_with_strategy(round_config.size, ToZero);
        let raw_price = price.round_dp_with_strategy(round_config.price, MidpointTowardZero);

        let raw_taker_amt = raw_maker_amt / raw_price;

        let raw_taker_amt = self.fix_amount_rounding(raw_taker_amt, round_config);

        (
            decimal_to_token_u32(raw_maker_amt),
            decimal_to_token_u32(raw_taker_amt),
        )
    }

    pub fn calculate_market_price(
        &self,
        positions: &[OrderSummary],
        amount_to_match: Decimal,
    ) -> Result<Decimal> {
        let mut sum = Decimal::ZERO;

        for p in positions {
            sum += p.size * p.price;
            if sum >= amount_to_match {
                return Ok(p.price);
            }
        }
        Err(anyhow!(
            "Not enough liquidity to create market order with amount {amount_to_match}"
        ))
    }

    pub fn create_market_order(
        &self,
        chain_id: u64,
        order_args: &MarketOrderArgs,
        price: Decimal,
        extras: &ExtraOrderArgs,
        options: CreateOrderOptions,
    ) -> Result<SignedOrderRequest> {
        let (maker_amount, taker_amount) = self.get_market_order_amounts(
            order_args.amount,
            price,
            &ROUNDING_CONFIG[&options
                .tick_size
                .context("Cannot create order without tick size")?],
        );

        let contract_config = get_contract_config(
            chain_id,
            options
                .neg_risk
                .context("Cannot create order without neg_risk")?,
        )
        .context("No contract found with given chain_id and neg_risk")?;

        let exchange_address = Address::from_str(contract_config.exchange.as_ref())
            .context("Invalid exchange address")?;

        self.build_signed_order(
            order_args.token_id.clone(),
            Side::BUY,
            chain_id,
            exchange_address,
            maker_amount,
            taker_amount,
            0,
            extras,
        )
    }

    pub fn create_order(
        &self,
        chain_id: u64,
        order_args: &OrderArgs,
        expiration: u64,
        extras: &ExtraOrderArgs,
        options: CreateOrderOptions,
    ) -> Result<SignedOrderRequest> {
        let (maker_amount, taker_amount) = self.get_order_amounts(
            order_args.side,
            order_args.size,
            order_args.price,
            &ROUNDING_CONFIG[&options
                .tick_size
                .context("Cannot create order without tick size")?],
        );

        let contract_config = get_contract_config(
            chain_id,
            options
                .neg_risk
                .context("Cannot create order without neg_risk")?,
        )
        .context("No contract found with given chain_id and neg_risk")?;

        let exchange_address = Address::from_str(contract_config.exchange.as_ref())
            .context("Invalid exchange address")?;

        self.build_signed_order(
            order_args.token_id.clone(),
            order_args.side,
            chain_id,
            exchange_address,
            maker_amount,
            taker_amount,
            expiration,
            extras,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_signed_order(
        &self,
        token_id: String,
        side: Side,
        chain_id: u64,
        exchange: Address,
        maker_amount: u32,
        taker_amount: u32,
        expiration: u64,
        extras: &ExtraOrderArgs,
    ) -> Result<SignedOrderRequest> {
        let seed = generate_seed();
        let taker_address =
            Address::from_str(extras.taker.as_ref()).context("Invalid taker address")?;

        let u256_token_id =
            U256::from_str_radix(token_id.as_ref(), 10).context("Incorrect tokenId format")?;

        let order = Order {
            salt: U256::from(seed),
            maker: self.funder,
            signer: self.signer.address(),
            taker: taker_address,
            tokenId: u256_token_id,
            makerAmount: U256::from(maker_amount),
            takerAmount: U256::from(taker_amount),
            expiration: U256::from(expiration),
            nonce: extras.nonce,
            feeRateBps: U256::from(extras.fee_rate_bps),
            side: side as u8,
            signatureType: self.sig_type as u8,
        };

        let signature = sign_order_message(&self.signer, order, chain_id, exchange)?;

        Ok(SignedOrderRequest {
            salt: seed,
            maker: self.funder.to_checksum(None),
            signer: self.signer.address().to_checksum(None),
            taker: taker_address.to_checksum(None),
            token_id,
            maker_amount: maker_amount.to_string(),
            taker_amount: taker_amount.to_string(),
            expiration: expiration.to_string(),
            nonce: extras.nonce.to_string(),
            fee_rate_bps: extras.fee_rate_bps.to_string(),
            side: side.as_str().into(),
            signature_type: self.sig_type as u8,
            signature,
        })
    }
}

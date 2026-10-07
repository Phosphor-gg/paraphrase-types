use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreditProductResponse {
    pub id: String,
    pub price_id: String,
    pub name: String,
    pub description: Option<String>,
    pub price_cents: i64,
    pub currency: String,
    pub credits_amount: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuyCreditsRequest {
    pub price_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuyCreditsResponse {
    pub checkout_url: String,
}

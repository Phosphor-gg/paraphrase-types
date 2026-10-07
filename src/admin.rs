use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateFlashSaleRequest {
    pub name: String,
    pub discount_percentage: i32,
    pub eligible_tiers: Option<Vec<String>>,
    pub starts_at: String,
    pub ends_at: String,
    pub max_uses: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashSaleResponse {
    pub id: String,
    pub name: String,
    pub discount_percentage: i32,
    pub stripe_coupon_id: String,
    pub eligible_tiers: Option<Vec<String>>,
    pub starts_at: String,
    pub ends_at: String,
    pub max_uses: Option<i32>,
    pub current_uses: i32,
    pub is_active: bool,
    pub created_at: String,
}

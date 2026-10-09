use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Display;

/// Length of the free trial, in days.
///
/// Shared so the number the pricing page promises is the number Stripe is
/// told to apply. It was stated in eleven places as a literal, which is how
/// copy and behaviour drift apart.
///
/// Seven rather than three because moderation proves itself by catching
/// something, and Discord traffic is weekly: a three day trial started on a
/// Tuesday ends before the weekend, which is exactly when a server is busiest.
/// It costs nothing extra to give, since a trialist already receives the
/// plan's whole monthly credit allowance either way.
pub const TRIAL_PERIOD_DAYS: u32 = 7;

/// Credits per 1 GBP. Based on £2.00 = 1,500,000 credits.
pub const CREDITS_PER_GBP: f64 = 750_000.0;

pub fn credits_to_gbp(credits: i64) -> f64 {
    credits as f64 / CREDITS_PER_GBP
}

pub fn format_credits_as_gbp(credits: i64) -> String {
    let gbp = credits_to_gbp(credits);
    let rounded_pence = (gbp * 100.0).round();
    if rounded_pence == rounded_pence.floor() && gbp >= 1.0 && (rounded_pence % 100.0) == 0.0 {
        format!("£{:.0}", gbp)
    } else {
        format!("£{:.2}", gbp)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreditBalance {
    pub used_current_period: i64,
    pub max_monthly_credits: i64,
    pub remaining_credits: i64,
    pub usage_percentage: f64,
    pub reset_date: Option<NaiveDateTime>,
    pub extra_credits: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreditTransaction {
    pub id: String,
    pub amount: i64,
    pub transaction_type: String,
    pub model_type: Option<String>,
    pub bytes_processed: Option<i64>,
    pub description: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StripeSubscriptionInfo {
    pub tier: Tier,
    pub billing_cycle: BillingCycle,
    pub status: String,
    pub current_period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub is_active: bool,
    pub price: Option<f64>, // Price in the smallest currency unit (e.g., pence, cents)
    pub currency: Option<String>, // Currency code (e.g., "gbp", "usd")
    pub payment_method_id: Option<String>, // Default payment method ID
    #[serde(default)]
    pub subscription_id: Option<String>,
    #[serde(default)]
    pub overdraft_enabled: bool,
    #[serde(default)]
    pub overdraft_limit: Option<i64>,
    #[serde(default)]
    pub overdraft_used: Option<i64>,
    /// Stripe entitlements for this customer.
    ///
    /// The dashboard gates features on these rather than on `tier`, matching
    /// what the API routes actually enforce. Deriving a feature from a tier
    /// duplicates that mapping, and the two disagree as soon as a feature moves
    /// product or is granted to a single customer.
    #[serde(default)]
    pub entitlements: Vec<crate::entitlements::Entitlement>,
}

impl StripeSubscriptionInfo {
    pub fn free() -> Self {
        Self {
            tier: Tier::Free,
            billing_cycle: BillingCycle::Monthly,
            status: "inactive".to_string(),
            current_period_end: None,
            cancel_at_period_end: false,
            is_active: false,
            price: None,
            currency: None,
            payment_method_id: None,
            subscription_id: None,
            overdraft_enabled: false,
            overdraft_limit: None,
            overdraft_used: None,
            entitlements: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverdraftSettingsRequest {
    pub enabled: bool,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverdraftSettingsResponse {
    pub enabled: bool,
    pub limit: i64,
    pub used: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionInfo {
    pub tier: Tier,
    pub cycle: BillingCycle,
    pub price: f64,
    pub expires_at: Option<NaiveDateTime>,
    pub max_monthly_credits: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditsInfoResponse {
    pub balance: i64,
    pub monthly_allocation: i64,
    pub used_this_month: i64,
    pub remaining_this_month: i64,
    pub usage_percentage: f64,
    #[serde(default)]
    pub reset_date: Option<NaiveDateTime>,
    #[serde(default)]
    pub extra_credits: i64,
    #[serde(default)]
    pub discount_available: bool,
    #[serde(default)]
    pub discount_expires_at: Option<String>,
    /// Percentage value of the personal discount (e.g. 10 means 10% off).
    /// Only meaningful when `discount_available` is true.
    #[serde(default)]
    pub discount_percentage: Option<u32>,
    /// Whether the user has exhausted their current short-window rate limit.
    #[serde(default)]
    pub rate_limited: bool,
    /// When the current rate-limit window resets (RFC3339), if rate limited.
    #[serde(default)]
    pub rate_limit_resets_at: Option<String>,
    /// Whether the free Premium trial can still be started (no paid plan and
    /// the one-per-account trial hasn't been used yet). Length is
    /// [`TRIAL_PERIOD_DAYS`].
    #[serde(default)]
    pub trial_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyUsageEntry {
    pub day: String,
    pub credits_used: i64,
}

#[derive(Debug, Deserialize)]
pub struct DailyUsageQuery {
    pub days: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCheckoutSessionRequest {
    pub tier: Tier,
    pub billing_cycle: BillingCycle,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referral_code: Option<String>,
    /// Start a free trial instead of an immediate charge: [`TRIAL_PERIOD_DAYS`]
    /// of Premium, card required, converting to a normal recurring
    /// subscription on the chosen billing cycle when it ends. Only for accounts
    /// with no active subscription, and only on cycles where
    /// [`BillingCycle::trial_eligible`] holds.
    #[serde(default)]
    pub trial: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCheckoutSessionResponse {
    pub checkout_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscriptionUpdateResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChangePlanRequest {
    pub tier: Tier,
    pub billing_cycle: BillingCycle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentMethodResponse {
    pub last4: String,
    pub brand: String,
    pub exp_month: u32,
    pub exp_year: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BillingPortalResponse {
    pub portal_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToggleAutoRenewalRequest {
    pub enable: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToggleAutoRenewalResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceInfo {
    pub price_id: String,
    pub product_id: String,
    pub tier: Tier,
    pub billing_cycle: BillingCycle,
    pub amount: i64, // Amount in cents
    pub currency: String,
    pub payment_link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormattedPrice {
    pub price_id: String,
    pub amount_cents: i64,
    pub amount_display: f64, // Amount in major currency units (e.g., dollars)
    pub currency: String,
}

impl FormattedPrice {
    pub fn currency_symbol(&self) -> &str {
        match self.currency.to_uppercase().as_str() {
            "USD" => "$",
            "EUR" => "€",
            "GBP" => "£",
            "JPY" => "¥",
            "CAD" => "CA$",
            "AUD" => "A$",
            "CHF" => "CHF ",
            "CNY" => "¥",
            "INR" => "₹",
            _ => &self.currency,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TierPricing {
    pub tier: Tier,
    pub prices: HashMap<BillingCycle, FormattedPrice>,
    #[serde(default)]
    pub monthly_credits: Option<i64>,
    #[serde(default)]
    pub feature_names: Vec<String>,
}

impl TierPricing {
    pub fn description(&self) -> String {
        self.tier.get_description().to_string()
    }

    /// The features Stripe says this plan includes.
    ///
    /// Empty when Stripe has not been configured, and deliberately so. There
    /// used to be a hardcoded fallback here, which meant an unconfigured Stripe
    /// rendered a pricing page claiming features the backend does not grant.
    /// A pricing page showing nothing is recoverable; one making promises
    /// nobody can keep is not.
    pub fn features(&self) -> Vec<String> {
        self.feature_names.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlashSaleInfo {
    pub id: String,
    pub name: String,
    pub discount_percentage: i32,
    pub eligible_tiers: Option<Vec<Tier>>,
    pub ends_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingData {
    pub tiers: Vec<TierPricing>,
    pub billing_cycles: Vec<BillingCycleInfo>,
    #[serde(default)]
    pub flash_sale: Option<FlashSaleInfo>,
    #[serde(default)]
    pub free_tier_credits: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BillingCycleInfo {
    pub cycle: BillingCycle,
    pub display_name: String,
    pub period_suffix: String,
    pub discount_percentage: Option<i32>,
}

/// Subscription tiers.
///
/// One plan, sold on several billing cycles. There is no ladder and no legacy
/// tier, because nothing has ever been sold on this product: the inherited
/// Basic and Standard existed only to keep a sibling product's subscriptions
/// resolving, and carrying them here would be carrying someone else's history.
///
/// `Ord` follows the variant order, so Free is below Premium and an upgrade
/// comparison needs no table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Hash, Eq, Copy, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Free,
    Premium,
}

impl Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Tier::Free => "Free",
            Tier::Premium => "Premium",
        };
        write!(f, "{name}")
    }
}

impl Tier {
    /// Resolve a stored or Stripe-supplied tier name.
    ///
    /// Normalised first, because a name arrives from Stripe metadata and from
    /// stored subscription rows in whatever shape someone typed it. Without
    /// this, "Premium " or "premium_plan" falls through to the default and a
    /// paying customer is silently served the free plan.
    pub fn parse(name: &str) -> Option<Tier> {
        let normalised: String = name
            .trim()
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        match normalised.as_str() {
            "free" => Some(Tier::Free),
            "premium" | "premiumplan" | "paid" => Some(Tier::Premium),
            _ => None,
        }
    }
}

impl From<String> for Tier {
    fn from(s: String) -> Self {
        Tier::from(s.as_str())
    }
}

impl From<&str> for Tier {
    /// Unrecognised names resolve to Free, which is the safe direction: a
    /// typo must never grant a paid plan. `parse` is the form to use where the
    /// difference matters.
    fn from(s: &str) -> Self {
        Tier::parse(s).unwrap_or(Tier::Free)
    }
}

#[derive(Clone, Debug, PartialEq, Copy, Serialize, Deserialize, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum BillingCycle {
    Monthly,
    Quarterly,
    Annual,
    Triennial,
}

impl BillingCycle {
    /// Whether a free trial can be started on this cycle.
    ///
    /// Triennial is excluded deliberately: three free days converting into a
    /// three year commitment is the shape most likely to end in a dispute or a
    /// chargeback, and the trial exists to de-risk trying Supervisor, not to
    /// front a long contract.
    ///
    /// Lives here rather than in the checkout route because the pricing page
    /// decides which cards offer a trial and the backend decides whether to
    /// grant one. If those two disagree, a customer is either shown an offer
    /// that is refused or charged when they expected a trial.
    pub fn trial_eligible(&self) -> bool {
        !matches!(self, BillingCycle::Triennial)
    }

    /// Months covered by one billing period.
    ///
    /// This is the ordering used to tell an upgrade from a downgrade: a longer
    /// term is an upgrade and prorates immediately, a shorter one is a
    /// downgrade and takes effect when the paid term ends. Note the money runs
    /// the other way (a longer term is cheaper per month), so comparing prices
    /// instead would call shortening a commitment an upgrade and bill for it.
    pub fn months(&self) -> u32 {
        match self {
            BillingCycle::Monthly => 1,
            BillingCycle::Quarterly => 3,
            BillingCycle::Annual => 12,
            BillingCycle::Triennial => 36,
        }
    }

    /// Whether moving from `self` to `target` shortens the commitment.
    pub fn is_downgrade_to(&self, target: BillingCycle) -> bool {
        target.months() < self.months()
    }

    pub fn period_suffix(&self) -> &str {
        match self {
            BillingCycle::Monthly => "month",
            BillingCycle::Quarterly => "3 months",
            BillingCycle::Annual => "year",
            BillingCycle::Triennial => "3 years",
        }
    }
    pub fn get_all_cycles() -> Vec<BillingCycle> {
        vec![
            BillingCycle::Monthly,
            BillingCycle::Quarterly,
            BillingCycle::Annual,
            BillingCycle::Triennial,
        ]
    }
    pub fn stripe_interval(&self) -> &'static str {
        match self {
            BillingCycle::Monthly => "month",
            BillingCycle::Quarterly => "month",
            BillingCycle::Annual => "year",
            BillingCycle::Triennial => "year",
        }
    }

    pub fn stripe_interval_count(&self) -> u32 {
        match self {
            BillingCycle::Monthly => 1,
            BillingCycle::Quarterly => 3,
            BillingCycle::Annual => 1,
            BillingCycle::Triennial => 3,
        }
    }
}

impl Display for BillingCycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cycle_str = match self {
            BillingCycle::Monthly => "Monthly",
            BillingCycle::Quarterly => "Quarterly",
            BillingCycle::Annual => "Annual",
            BillingCycle::Triennial => "Triennial",
        };
        write!(f, "{}", cycle_str)
    }
}

impl From<String> for BillingCycle {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "monthly" => BillingCycle::Monthly,
            "quarterly" => BillingCycle::Quarterly,
            "annual" => BillingCycle::Annual,
            "triennial" => BillingCycle::Triennial,
            _ => BillingCycle::Monthly, // Default to Monthly for invalid values
        }
    }
}

impl From<&str> for BillingCycle {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "monthly" => BillingCycle::Monthly,
            "quarterly" => BillingCycle::Quarterly,
            "annual" => BillingCycle::Annual,
            "triennial" => BillingCycle::Triennial,
            _ => BillingCycle::Monthly,
        }
    }
}

impl Tier {
    /// Every tier that exists.
    pub fn all_tiers() -> Vec<Tier> {
        vec![Tier::Free, Tier::Premium]
    }

    /// Whether this tier is sold.
    pub fn is_paid(&self) -> bool {
        matches!(self, Tier::Premium)
    }

    pub fn get_description(&self) -> &str {
        match self {
            Tier::Free => "Record, transcribe and edit by hand.",
            Tier::Premium => "Everything, including one-button cleanup.",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferralStats {
    pub referral_code: String,
    pub total_referrals: usize,
    /// Referrals that have paid out, i.e. the referred user subscribed.
    pub rewarded_referrals: usize,
    /// Credits earned from referrals, all time. Render with
    /// [`format_credits_as_gbp`]; the raw credit figure is never shown.
    pub total_reward_credits: i64,
    pub referral_breakdown: Vec<ReferralInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferralInfo {
    pub user_id: String,
    pub created_at: String,
    /// Credits this referral paid out, or 0 while it is still pending.
    pub reward_credits: i64,
    /// Whether the reward has been paid. A referral pays out once, when the
    /// referred user first subscribes, so this never returns to false even if
    /// they later cancel.
    pub rewarded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceInfo {
    pub id: String,
    pub amount: i64,
    pub currency: String,
    pub status: String,
    pub created: String,
    pub hosted_invoice_url: Option<String>,
    pub invoice_pdf: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoicesResponse {
    pub invoices: Vec<InvoiceInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_one_paid_plan() {
        assert_eq!(Tier::all_tiers(), vec![Tier::Free, Tier::Premium]);
        assert!(Tier::Premium.is_paid());
        assert!(!Tier::Free.is_paid());
    }

    #[test]
    fn a_tier_name_is_normalised_before_being_matched() {
        // These arrive from Stripe metadata and stored subscription rows in
        // whatever shape someone typed. Falling through to Free on any of them
        // would serve a paying customer the free plan.
        for name in ["Premium", "premium", " premium ", "PREMIUM", "premium_plan", "Premium-Plan"] {
            assert_eq!(Tier::parse(name), Some(Tier::Premium), "{name:?}");
        }
        for name in ["Free", " free", "FREE"] {
            assert_eq!(Tier::parse(name), Some(Tier::Free), "{name:?}");
        }
    }

    #[test]
    fn an_unknown_tier_name_is_reported_rather_than_guessed() {
        assert_eq!(Tier::parse("enterprise"), None);
        assert_eq!(Tier::parse(""), None);
        // The lossy conversion still exists for stored values, and errs
        // towards Free, because a typo must never grant a paid plan.
        assert_eq!(Tier::from("enterprise"), Tier::Free);
    }

    #[test]
    fn a_tier_round_trips_through_its_wire_form() {
        for t in Tier::all_tiers() {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(json, format!("\"{}\"", t.to_string().to_lowercase()));
            assert_eq!(serde_json::from_str::<Tier>(&json).unwrap(), t);
        }
    }

    #[test]
    fn premium_outranks_free_without_a_lookup_table() {
        assert!(Tier::Premium > Tier::Free);
        let mut tiers = vec![Tier::Premium, Tier::Free];
        tiers.sort();
        assert_eq!(tiers, vec![Tier::Free, Tier::Premium]);
    }

    #[test]
    fn an_unconfigured_stripe_advertises_nothing() {
        // The bug this replaces: a hardcoded fallback meant an unconfigured
        // Stripe rendered a pricing page promising features the backend does
        // not grant.
        let pricing = TierPricing {
            tier: Tier::Premium,
            prices: std::collections::HashMap::new(),
            monthly_credits: None,
            feature_names: vec![],
        };
        assert!(pricing.features().is_empty());
    }

    #[test]
    fn features_come_from_stripe_verbatim() {
        let pricing = TierPricing {
            tier: Tier::Premium,
            prices: std::collections::HashMap::new(),
            monthly_credits: Some(36_000),
            feature_names: vec!["Clean up recordings with one button".to_string()],
        };
        assert_eq!(pricing.features(), vec!["Clean up recordings with one button"]);
    }

    #[test]
    fn a_description_describes_this_product() {
        for t in Tier::all_tiers() {
            let d = t.get_description();
            assert!(d.ends_with('.'), "{t:?}: {d}");
            for leak in ["communities", "moderation", "bot", "Discord", "server"] {
                assert!(!d.contains(leak), "{t:?} still describes another product: {d}");
            }
        }
    }


    #[test]
    fn every_cycle_but_triennial_can_be_trialled() {
        assert!(BillingCycle::Monthly.trial_eligible());
        assert!(BillingCycle::Quarterly.trial_eligible());
        assert!(BillingCycle::Annual.trial_eligible());
        // Three free days should not front a three year commitment.
        assert!(!BillingCycle::Triennial.trial_eligible());
    }

    #[test]
    fn direction_follows_term_length_not_price() {
        // Triennial is the cheapest per month, so a price comparison would call
        // this an upgrade. It shortens the commitment, so it is a downgrade.
        assert!(BillingCycle::Triennial.is_downgrade_to(BillingCycle::Annual));
        assert!(BillingCycle::Annual.is_downgrade_to(BillingCycle::Monthly));

        assert!(!BillingCycle::Monthly.is_downgrade_to(BillingCycle::Annual));
        assert!(!BillingCycle::Annual.is_downgrade_to(BillingCycle::Triennial));
        // Same cycle is not a downgrade.
        assert!(!BillingCycle::Annual.is_downgrade_to(BillingCycle::Annual));
    }
}

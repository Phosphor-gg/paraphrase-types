use serde::{Deserialize, Serialize};

use crate::pricing::{BillingCycle, Tier};

// Registration

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformRegistrationRequest {
    pub name: String,
    pub redirect_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_primary_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_accent_color: Option<String>,
    /// ISO 3166-1 alpha-2 country for the Stripe Connect payout account.
    /// Must be a region Stripe supports for cross-border destination-charge
    /// transfers (US, UK, EEA, CA, CH). Defaults to the platform account's
    /// country when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformRegistrationResponse {
    pub platform_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub stripe_onboarding_url: Option<String>,
}

// Token exchange (OAuth2 client_credentials)

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformTokenRequest {
    pub client_id: String,
    pub client_secret: String,
    pub grant_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
}

// User provisioning

#[derive(Debug, Serialize, Deserialize)]
pub struct ProvisionUserRequest {
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProvisionUserResponse {
    pub user_id: String,
    pub email: String,
    pub is_new_account: bool,
    pub is_newly_linked: bool,
}

// Checkout

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformCheckoutRequest {
    pub user_email: String,
    pub tier: Tier,
    pub billing_cycle: BillingCycle,
    pub success_url: String,
    pub cancel_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformCheckoutResponse {
    pub checkout_url: String,
}

// Products a platform can sell to its linked users. Plan payment_link is
// always None here: platforms must mint links via the checkout endpoints so
// the revenue share applies.

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformProductsResponse {
    pub plans: Vec<crate::pricing::PriceInfo>,
    pub credit_packs: Vec<crate::credits::CreditProductResponse>,
}

// Credit pack checkout (one-time payment, revenue share on the payment)

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformCreditCheckoutRequest {
    pub user_email: String,
    /// price_id of a credit pack from GET /api/platform/products
    pub price_id: String,
    pub success_url: String,
    pub cancel_url: String,
}

// Credits balance of an authorized linked user

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformUserCreditsResponse {
    pub user_id: String,
    pub email: String,
    /// Total usable credits right now (monthly remaining plus extra)
    pub balance: i64,
    pub monthly_allocation: i64,
    pub used_this_month: i64,
    pub remaining_this_month: i64,
    pub extra_credits: i64,
    pub reset_date: Option<String>,
}

// Plan change (existing subscription, same Stripe customer)

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformChangePlanRequest {
    pub user_email: String,
    pub tier: Tier,
    pub billing_cycle: BillingCycle,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformChangePlanResponse {
    pub subscription_id: String,
    pub tier: Tier,
    pub billing_cycle: BillingCycle,
}

// User info

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformUserInfo {
    pub user_id: String,
    pub email: String,
    pub linked_at: String,
    pub authorized: bool,
    pub has_active_subscription: bool,
    pub tier: Tier,
}

// Stripe Connect

#[derive(Debug, Serialize, Deserialize)]
pub struct StripeConnectStatusResponse {
    pub account_id: Option<String>,
    pub onboarding_complete: bool,
    pub charges_enabled: bool,
}

// Delegated moderation

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformModerationRequest {
    pub user_email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<crate::moderate::ModerationModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_labels: Option<Vec<crate::moderate::ModerationLabel>>,
    #[serde(default)]
    pub include_context: bool,
    #[serde(default)]
    pub include_implicit: bool,
}

// Consent / Authorization

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthorizePlatformPageData {
    pub platform_id: String,
    pub platform_name: String,
    pub logo_url: Option<String>,
    pub description: Option<String>,
    pub theme_primary_color: Option<String>,
    pub theme_accent_color: Option<String>,
    pub redirect_uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthorizePlatformResponse {
    pub redirect_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfirmAuthorizationRequest {
    pub code: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfirmAuthorizationResponse {
    pub user_id: String,
    pub email: String,
}

// Dashboard: authorized apps

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedAppInfo {
    pub platform_id: String,
    pub platform_name: String,
    pub logo_url: Option<String>,
    pub authorized_at: Option<String>,
}

// Dashboard: platform management (platforms the user owns)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformSummary {
    pub platform_id: String,
    pub client_id: String,
    pub name: String,
    /// First 12 chars of the secret (`sk_platform_`); the full secret is only
    /// ever returned once, at creation/regeneration.
    pub client_secret_prefix: String,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub redirect_uri: String,
    pub webhook_url: Option<String>,
    pub theme_primary_color: Option<String>,
    pub theme_accent_color: Option<String>,
    pub stripe_onboarding_complete: bool,
    pub has_stripe_connect: bool,
    pub is_active: bool,
    pub created_at: String,
}

/// Editable platform fields (everything except credentials). All fields are
/// replaced with the supplied values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformUpdateRequest {
    pub name: String,
    pub redirect_uri: String,
    pub webhook_url: Option<String>,
    pub logo_url: Option<String>,
    pub description: Option<String>,
    pub theme_primary_color: Option<String>,
    pub theme_accent_color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegenerateSecretResponse {
    pub client_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingLinkResponse {
    pub url: String,
}

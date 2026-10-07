use crate::moderate::{ModerationLabel, ModerationModel};
use crate::pricing::Tier;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize)]
pub struct GuildsInfoRequest {
    pub discord_id: String,
    pub admin_guild_ids: Vec<String>,
    pub guild_admin_ids: HashMap<String, Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GuildsInfoResponse {
    pub guilds: Vec<GuildInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GuildInfoRequest {
    pub discord_id: String,
    pub guild_admin_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct GuildInfo {
    pub id: String,
    pub name: String,
    pub owner_id: String,
    pub icon: Option<String>,
    pub channels: HashMap<String, ChannelInfo>,
    pub roles: HashMap<String, RoleInfo>,
    pub admins: HashMap<String, UserInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
    pub channel_type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct RoleInfo {
    pub id: String,
    pub name: String,
    pub color: u32,
    pub position: u16,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AdminInfo {
    pub user_info: UserInfo,
    pub subscription_tier: Tier,
    pub has_account: bool,
    /// Stripe entitlements resolved for this admin.
    ///
    /// The dashboard gates feature toggles on these rather than on
    /// `subscription_tier`, because the moderation routes gate on entitlements
    /// too. Inferring a feature from a tier duplicates the mapping in a second
    /// place, and the two disagree the moment a feature is attached to a
    /// different product or granted to one customer.
    #[serde(default)]
    pub entitlements: Vec<crate::entitlements::Entitlement>,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AdminConfig {
    pub is_opted_in: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AdminData {
    pub admin_info: AdminInfo,
    pub admin_config: AdminConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DiscordStatusResponse {
    pub discord_id: Option<String>,
    pub email: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UserInfo {
    pub discord_id: String,
    pub username: String,
    pub is_owner: bool,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordModerateRequest {
    pub guild_info: GuildInfo,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

/// Batch image moderation for a single message: all attached images moderated
/// in one round-trip. The response is a `Vec<ModerationResponse>` aligned 1:1
/// with `images`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordBatchModerateRequest {
    pub guild_info: GuildInfo,
    /// Base64-encoded images (no data: prefix), one per attachment.
    pub images: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordVideoModerateRequest {
    pub guild_info: GuildInfo,
    /// Base64-encoded video (no data: prefix), one attachment per request.
    pub video: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationFeedbackRequest {
    pub source: String,
    pub vote: String,
    pub original_text: String,
}

/// Re-run a message the guild's model flagged through Arbiter, so a moderator
/// who downvoted the flag can see what the top model would have said.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbiterPreviewRequest {
    pub guild_info: GuildInfo,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbiterPreviewResponse {
    /// The model the guild is actually moderating with, after entitlement
    /// gating and Auto resolution.
    pub current_model: crate::moderate::ModerationModel,
    /// Whether the guild's billing user can already use Arbiter, i.e. the fix
    /// is switching the model rather than upgrading the plan.
    pub has_arbiter: bool,
    /// Arbiter's verdict. `None` when the guild already runs Arbiter, since
    /// there is nothing better to compare against.
    pub arbiter: Option<crate::moderate::ModerationResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationFeedbackIngestResponse {
    pub stored: bool,
    pub dataset: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyCompleteRequest {
    pub guild_id: String,
    pub user_id: String,
    pub method: String,
    pub turnstile_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordDataResponse {
    pub discord_data: HashMap<String, DiscordData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscordData {
    pub guild_info: GuildInfo,
    pub guild_config: GuildConfig,
    pub admin_data: HashMap<String, AdminData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GuildContext {
    pub guild_config: GuildConfig,
    pub guild_admin_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoteStatusResponse {
    pub created_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub weight: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GuildConfig {
    #[serde(default = "default_moderate_all_channels")]
    pub moderate_all_channels: bool,
    pub moderated_channels: HashMap<String, ChannelInfo>,
    pub enabled_labels: HashSet<ModerationLabel>,
    #[serde(default = "default_moderate_all_roles")]
    pub moderate_all_roles: bool,
    #[serde(default = "default_role_filter_mode")]
    pub role_filter_mode: RoleFilterMode,
    pub filtered_roles: HashMap<String, RoleInfo>,
    pub actions: HashSet<ModerationAction>,
    #[serde(default = "default_is_active")]
    pub is_active: bool,
    #[serde(default = "default_model")]
    pub model: ModerationModel,
    #[serde(rename = "alerts_channel_id")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alerts_channel: Option<String>,
    #[serde(default = "default_context_history_count")]
    pub context_history_count: i32,
    #[serde(default = "default_enable_context")]
    pub enable_context: bool,
    #[serde(default)]
    pub enable_implicit_labels: bool,
    // Admin intent for image moderation, stored raw (never rewritten by
    // entitlement state). Defaults true so entitled guilds get it on
    // automatically. Entitlement is enforced at runtime by the moderation
    // routes on the resolved billing user, exactly like enable_context and
    // enable_implicit_labels.
    #[serde(default = "default_true")]
    pub enable_image_moderation: bool,
    // Separate from images and defaults off: a video costs several frames
    // where an image costs one, so it is opted into rather than inherited.
    #[serde(default)]
    pub enable_video_moderation: bool,
    #[serde(default = "default_timeout_duration_minutes")]
    pub timeout_duration_minutes: i32,
    #[serde(default = "default_true")]
    pub enable_link_filter: bool,
    #[serde(default = "default_true")]
    pub block_discord_invites: bool,
    #[serde(default)]
    pub block_discord_media: bool,
    #[serde(default)]
    pub block_discord_nitro: bool,
    #[serde(default = "default_link_filter_mode")]
    pub link_filter_mode: LinkFilterMode,
    #[serde(default)]
    pub custom_link_filters: Vec<String>,
    /// Custom word filter: block messages containing configured words or
    /// phrases before AI moderation (blacklist-only counterpart to the
    /// custom link filters).
    #[serde(default)]
    pub enable_word_filter: bool,
    /// Words/phrases to block. Matched case-insensitively on word
    /// boundaries; a leading or trailing `*` allows partial matches
    /// (e.g. `spoiler*`).
    #[serde(default)]
    pub custom_word_filters: Vec<String>,
    #[serde(default)]
    pub enable_verification: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_role_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_channel_id: Option<String>,
    #[serde(default)]
    pub enable_username_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendVerifyMessageRequest {
    pub channel_id: String,
}

/// Bot "Check Status" button: ask the backend for the operational status of
/// a user's account and the servers they administrate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotStatusRequest {
    pub user_discord_id: String,
    pub guilds: Vec<BotStatusGuildQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotStatusGuildQuery {
    pub guild_id: String,
    pub guild_name: String,
    /// Discord ids of resolved guild admins (bot-side permission check);
    /// the backend maps these to Supervisor accounts for billing.
    pub admin_discord_ids: Vec<String>,
    /// Guild owner's Discord id: billing eligibility treats the owner
    /// specially (default opted-in), so status must mirror that.
    pub owner_discord_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotStatusResponse {
    pub account: BotAccountStatus,
    pub guilds: Vec<BotGuildStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotAccountStatus {
    /// Whether the requesting Discord user has a linked Supervisor account.
    pub linked: bool,
    pub remaining_credits: i64,
    pub monthly_allocation: i64,
    pub extra_credits: i64,
    pub rate_limited: bool,
    /// RFC3339, present when rate_limited.
    pub rate_limit_resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotGuildStatus {
    pub guild_id: String,
    pub guild_name: String,
    pub active: bool,
    /// Why moderation is not running, when inactive.
    pub reason: Option<BotGuildInactiveReason>,
    /// RFC3339 reset time when the reason is RateLimited.
    #[serde(default)]
    pub rate_limit_resets_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BotGuildInactiveReason {
    ConfigDisabled,
    NoAdminAccount,
    NoCredits,
    RateLimited,
}

fn default_true() -> bool {
    true
}

fn default_moderate_all_channels() -> bool {
    true
}

fn default_is_active() -> bool {
    true
}

fn default_moderate_all_roles() -> bool {
    true
}

fn default_enable_context() -> bool {
    false
}

fn default_role_filter_mode() -> RoleFilterMode {
    RoleFilterMode::Exclude
}

fn default_model() -> ModerationModel {
    ModerationModel::Auto
}

fn default_context_history_count() -> i32 {
    5
}

fn default_timeout_duration_minutes() -> i32 {
    5
}

fn default_link_filter_mode() -> LinkFilterMode {
    LinkFilterMode::Blacklist
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum LinkFilterMode {
    Whitelist,
    Blacklist,
}

impl FromStr for LinkFilterMode {
    type Err = ();

    fn from_str(input: &str) -> Result<LinkFilterMode, Self::Err> {
        match input {
            "whitelist" => Ok(LinkFilterMode::Whitelist),
            "blacklist" => Ok(LinkFilterMode::Blacklist),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ModerationAction {
    Delete,
    Timeout,
    Warn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RoleFilterMode {
    Include,
    Exclude,
}

impl Default for GuildConfig {
    fn default() -> Self {
        Self {
            moderate_all_channels: default_moderate_all_channels(),
            moderated_channels: HashMap::new(),
            enabled_labels: HashSet::from([
                ModerationLabel::H,
                ModerationLabel::HR,
                ModerationLabel::S,
                ModerationLabel::SU,
                ModerationLabel::S2,
                ModerationLabel::SE,
                ModerationLabel::V,
                ModerationLabel::SH,
                ModerationLabel::SP,
                ModerationLabel::PM,
                ModerationLabel::SI,
                ModerationLabel::IL,
            ]),
            moderate_all_roles: default_moderate_all_roles(),
            role_filter_mode: default_role_filter_mode(),
            filtered_roles: HashMap::new(),
            actions: HashSet::from([ModerationAction::Delete]),
            is_active: default_is_active(),
            model: default_model(),
            alerts_channel: None,
            context_history_count: default_context_history_count(),
            enable_context: default_enable_context(),
            enable_implicit_labels: false,
            enable_image_moderation: true,
            enable_video_moderation: false,
            timeout_duration_minutes: default_timeout_duration_minutes(),
            enable_link_filter: true,
            block_discord_invites: true,
            block_discord_media: false,
            block_discord_nitro: false,
            link_filter_mode: default_link_filter_mode(),
            custom_link_filters: Vec::new(),
            enable_word_filter: false,
            custom_word_filters: Vec::new(),
            enable_verification: false,
            verified_role_id: None,
            verify_channel_id: None,
            enable_username_check: false,
        }
    }
}

impl Default for GuildContext {
    fn default() -> Self {
        GuildContext {
            guild_config: Default::default(),
            guild_admin_ids: vec![],
        }
    }
}

impl FromStr for ModerationAction {
    type Err = ();

    fn from_str(input: &str) -> Result<ModerationAction, Self::Err> {
        match input {
            "delete" => Ok(ModerationAction::Delete),
            "timeout" => Ok(ModerationAction::Timeout),
            "warn" => Ok(ModerationAction::Warn),
            _ => Err(()),
        }
    }
}

impl FromStr for RoleFilterMode {
    type Err = ();

    fn from_str(input: &str) -> Result<RoleFilterMode, Self::Err> {
        match input {
            "include" => Ok(RoleFilterMode::Include),
            "exclude" => Ok(RoleFilterMode::Exclude),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for ModerationAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModerationAction::Delete => write!(f, "delete"),
            ModerationAction::Timeout => write!(f, "timeout"),
            ModerationAction::Warn => write!(f, "warn"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotAppearanceRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotAppearanceResponse {
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub bio: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountLinkedNotification {
    pub discord_id: String,
    pub is_new_account: bool,
}

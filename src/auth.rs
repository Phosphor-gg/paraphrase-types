use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub discord_id: Option<String>,
    pub username: Option<String>,
    pub global_name: Option<String>,
    pub avatar: Option<String>,
    pub exp: usize, // expiration time
    pub iat: usize, // issued at
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackQuery {
    pub code: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionInitiateResponse {
    pub auth_url: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionCallbackQuery {
    pub code: String,
    pub state: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkDiscordQuery {
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthInitiateResponse {
    pub auth_url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordCallbackQuery {
    pub code: String,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiKeyResponse {
    pub id: String,
    pub name: String,
    pub key_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyResponse {
    pub id: String,
    pub name: String,
    pub full_key: String,
    pub key_preview: String,
    pub warning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteApiKeyResponse {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
}

#[derive(Debug)]
pub struct GeneratedApiKey {
    pub id: Uuid,
    pub full_key: String,
    pub name: String,
    pub key_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub refresh_token: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordUser {
    pub id: String,
    pub username: String,
    pub discriminator: Option<String>,
    pub global_name: Option<String>,
    pub avatar: Option<String>,
    pub email: String,
    /// Whether Discord has verified this email. Default false so a missing
    /// field is treated as unverified — account matching by email must only
    /// trust verified emails.
    #[serde(default)]
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub refresh_token: Option<String>,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleUser {
    pub id: String,
    pub email: String,
    pub verified_email: bool,
    pub name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub picture: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubEmail {
    pub email: String,
    pub primary: bool,
    pub verified: bool,
    pub visibility: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserInfo,
    #[serde(default)]
    pub is_new_account: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInfo {
    pub id: String,
    pub discord_id: Option<String>,
    pub email: String,
    pub username: String,
    pub global_name: Option<String>,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRequest<'a> {
    pub client_id: &'a str,
    pub client_secret: &'a str,
    pub grant_type: &'a str,
    pub code: &'a str,
    pub redirect_uri: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProviderType {
    Discord,
    Google,
    GitHub,
}


impl OAuthProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            OAuthProviderType::Discord => "discord",
            OAuthProviderType::Google => "google",
            OAuthProviderType::GitHub => "github",
        }
    }
}

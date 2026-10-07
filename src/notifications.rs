use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationResponse {
    pub id: String,
    pub title: String,
    pub message: String,
    pub notification_type: String,
    pub created_at: String,
    pub is_read: bool,
    pub is_important: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BroadcastNotificationRequest {
    pub title: String,
    pub message: String,
    #[serde(default = "default_notification_type")]
    pub notification_type: String,
    #[serde(default)]
    pub is_important: Option<bool>,
}

fn default_notification_type() -> String {
    "announcement".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationPreferencesResponse {
    pub marketing_notifications_enabled: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateNotificationPreferencesRequest {
    pub marketing_notifications_enabled: bool,
}
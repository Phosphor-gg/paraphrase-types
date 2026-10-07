use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Entitlement {
    ObserverModel,
    SentinelModel,
    ArbiterModel,
    Context,
    // Stripe feature key: "implicit-moderation" (renamed from the legacy
    // "implicit-labels" key when image moderation joined the lineup, so both
    // moderation-feature keys share the "*-moderation" naming).
    ImplicitLabels,
    // Stripe feature key: "image-moderation". Gates whether image base64 sent
    // to /moderate or /batch is processed by the OCR + SigLIP pipeline.
    ImageModeration,
    // Stripe feature key: "video-moderation". Gates /moderate/video, which
    // extracts keyframes and runs them through the image pipeline.
    VideoModeration,
    CustomBotAppearance,
    PlatformApi,
}

impl Entitlement {
    pub fn lookup_key(&self) -> &'static str {
        match self {
            Self::ObserverModel => "observer-model",
            Self::SentinelModel => "sentinel-model",
            Self::ArbiterModel => "arbiter-model",
            Self::Context => "context",
            Self::ImplicitLabels => "implicit-moderation",
            Self::ImageModeration => "image-moderation",
            Self::VideoModeration => "video-moderation",
            Self::CustomBotAppearance => "custom-bot-appearance",
            Self::PlatformApi => "platform-api",
        }
    }

    pub fn from_lookup_key(key: &str) -> Option<Self> {
        match key {
            "observer-model" => Some(Self::ObserverModel),
            "sentinel-model" => Some(Self::SentinelModel),
            "arbiter-model" => Some(Self::ArbiterModel),
            "context" => Some(Self::Context),
            "implicit-moderation" => Some(Self::ImplicitLabels),
            "image-moderation" => Some(Self::ImageModeration),
            "video-moderation" => Some(Self::VideoModeration),
            "custom-bot-appearance" => Some(Self::CustomBotAppearance),
            "platform-api" => Some(Self::PlatformApi),
            _ => None,
        }
    }
}

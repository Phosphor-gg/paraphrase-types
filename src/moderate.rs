use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModerationModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_labels: Option<Vec<ModerationLabel>>,
    #[serde(default)]
    pub include_context: bool,
    #[serde(default)]
    pub include_implicit: bool,
}

/// A short video to moderate. Keyframes are extracted and run through the same
/// pipeline as images; there is no video-specific model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoModerationRequest {
    /// Base64-encoded video, optionally with a `data:video/...;base64,` prefix.
    pub video: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModerationModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_labels: Option<Vec<ModerationLabel>>,
    #[serde(default)]
    pub include_implicit: bool,
}

/// One analysed frame, with the point in the clip it came from so a caller can
/// seek straight to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFrameResult {
    pub timestamp_ms: i64,
    pub flagged: bool,
    pub labels: Vec<ModerationLabel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoModerationResponse {
    /// True when any analysed frame was flagged.
    pub flagged: bool,
    /// Union of the labels across every analysed frame.
    pub labels: Vec<ModerationLabel>,
    pub frames: Vec<VideoFrameResult>,
    pub frames_analysed: usize,
    pub duration_secs: f64,
    /// Container codec as detected, e.g. "h264" or "vp90".
    pub codec: String,
    /// Which decoder handled it, "nvdec" or "cpu". Useful for spotting a
    /// silent fall back to software decode.
    pub decoder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchModerationRequest {
    #[serde(default)]
    pub texts: Vec<String>,
    #[serde(default)]
    pub images: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModerationModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_labels: Option<Vec<ModerationLabel>>,
    #[serde(default)]
    pub include_context: bool,
    #[serde(default)]
    pub include_implicit: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationResponse {
    pub flagged: bool,
    pub labels: Vec<ModerationLabel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implicit_labels: Option<Vec<ModerationLabel>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_context: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_labels: Option<Vec<ModerationLabel>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rewritten_text: Option<String>,
    /// OCR text extracted from an image request (None for text-only).
    /// Shown on flagged-image alerts and used for image moderation feedback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationLogEntry {
    pub text: String,
    pub result: String,
    pub timestamp: String,
}

impl ModerationLogEntry {
    pub fn now(text: String, result: String) -> Self {
        Self {
            text,
            result,
            timestamp: Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ModerationLabel {
    #[serde(rename = "profanity", alias = "P")]
    P,
    #[serde(rename = "toxicity", alias = "T")]
    T,
    #[serde(rename = "harassment", alias = "H")]
    H,
    #[serde(rename = "hate", alias = "HR")]
    HR,
    #[serde(rename = "insult", alias = "I")]
    I,
    #[serde(rename = "sexual", alias = "S")]
    S,
    #[serde(rename = "sexual/unlawful", alias = "SU", alias = "sexual/minors")]
    SU,
    #[serde(rename = "sexual/explicit", alias = "S2")]
    S2,
    #[serde(rename = "sensitive", alias = "SE")]
    SE,
    #[serde(rename = "violence", alias = "V")]
    V,
    #[serde(rename = "self-harm", alias = "SH")]
    SH,
    #[serde(rename = "medical", alias = "M")]
    M,
    #[serde(rename = "spam", alias = "SP")]
    SP,
    #[serde(rename = "promotional", alias = "PM")]
    PM,
    #[serde(rename = "scam", alias = "SI")]
    SI,
    #[serde(rename = "illegal", alias = "IL")]
    IL,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ModerationModel {
    Auto,
    Observer,
    Sentinel,
    Arbiter,
}

impl Display for ModerationModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModerationModel::Auto => write!(f, "auto"),
            ModerationModel::Observer => write!(f, "observer"),
            ModerationModel::Sentinel => write!(f, "sentinel"),
            ModerationModel::Arbiter => write!(f, "arbiter"),
        }
    }
}

impl Display for ModerationLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModerationLabel::P => write!(f, "P"),
            ModerationLabel::T => write!(f, "T"),
            ModerationLabel::H => write!(f, "H"),
            ModerationLabel::HR => write!(f, "HR"),
            ModerationLabel::I => write!(f, "I"),
            ModerationLabel::S => write!(f, "S"),
            ModerationLabel::SU => write!(f, "SU"),
            ModerationLabel::S2 => write!(f, "S2"),
            ModerationLabel::SE => write!(f, "SE"),
            ModerationLabel::V => write!(f, "V"),
            ModerationLabel::SH => write!(f, "SH"),
            ModerationLabel::M => write!(f, "M"),
            ModerationLabel::SP => write!(f, "SP"),
            ModerationLabel::PM => write!(f, "PM"),
            ModerationLabel::SI => write!(f, "SI"),
            ModerationLabel::IL => write!(f, "IL"),
        }
    }
}

impl FromStr for ModerationLabel {
    type Err = ();

    fn from_str(input: &str) -> Result<ModerationLabel, Self::Err> {
        match input {
            "P" => Ok(ModerationLabel::P),
            "T" => Ok(ModerationLabel::T),
            "H" => Ok(ModerationLabel::H),
            "HR" => Ok(ModerationLabel::HR),
            "I" => Ok(ModerationLabel::I),
            "S" => Ok(ModerationLabel::S),
            "SU" => Ok(ModerationLabel::SU),
            "S2" => Ok(ModerationLabel::S2),
            "SE" => Ok(ModerationLabel::SE),
            "V" => Ok(ModerationLabel::V),
            "SH" => Ok(ModerationLabel::SH),
            "M" => Ok(ModerationLabel::M),
            "SP" => Ok(ModerationLabel::SP),
            "PM" => Ok(ModerationLabel::PM),
            "SI" => Ok(ModerationLabel::SI),
            "IL" => Ok(ModerationLabel::IL),
            _ => Err(()),
        }
    }
}

impl FromStr for ModerationModel {
    type Err = ();

    fn from_str(input: &str) -> Result<ModerationModel, Self::Err> {
        match input.to_lowercase().as_str() {
            "auto" => Ok(ModerationModel::Auto),
            "observer" => Ok(ModerationModel::Observer),
            "sentinel" => Ok(ModerationModel::Sentinel),
            "arbiter" => Ok(ModerationModel::Arbiter),
            _ => Err(()),
        }
    }
}

/// Image bytes are billed at this flat rate regardless of model. Text the
/// OCR extracts from an image is billed separately at the model's
/// credits_per_byte once moderation actually runs it through a text model.
pub const IMAGE_CREDITS_PER_BYTE: i64 = 1;

/// Video frames are billed below the flat image rate. A clip yields up to
/// `MAX_VIDEO_FRAMES` frames, so charging each at the image rate would make one
/// video cost more than a month of the entry plan. Frames are also extracted at
/// a smaller edge than a standalone image (the vision model sees 224px either
/// way), so they are both cheaper per byte and fewer bytes.
pub const VIDEO_FRAME_CREDITS_PER_BYTE_PERCENT: i64 = 25;

/// Batched requests are discounted: one GPU forward covers the whole batch
/// instead of one per item, and that saving is passed on.
pub const BATCH_DISCOUNT_PERCENT: i64 = 50;

/// The discount only applies from this batch size up. A batch of one costs
/// exactly what a single request costs to serve, so discounting it would just
/// be a 50% price cut that any caller could take by wrapping every request in a
/// one-item batch.
pub const MIN_BATCH_SIZE_FOR_DISCOUNT: usize = 2;

/// Hard limits on a single video. Enforced server-side and mirrored in the SDKs
/// so callers fail before uploading.
pub const MAX_VIDEO_BYTES: i64 = 10 * 1024 * 1024;
pub const MAX_VIDEO_DURATION_SECS: f64 = 60.0;
pub const MAX_VIDEO_FRAMES: usize = 20;

/// Frame extraction contract.
///
/// These are not implementation details of one service: the SDKs and the Discord
/// bot extract frames locally and send them to the batch endpoint, so every
/// implementation has to agree or the same clip costs and scores differently
/// depending on which one processed it. Frame size in particular feeds directly
/// into billing, since frames are charged by byte.
///
/// Smaller than a standalone image (1280px) on purpose: the vision model sees
/// 224px regardless, so beyond ~720px the extra pixels buy only OCR legibility
/// while costing credits linearly.
pub const VIDEO_FRAME_MAX_EDGE: u32 = 720;
pub const VIDEO_FRAME_JPEG_QUALITY: u8 = 80;

/// How often to pull a candidate frame before deduplication, in milliseconds.
/// Fine enough not to miss a short shot; dedup collapses the redundancy.
pub const VIDEO_SAMPLE_INTERVAL_MS: u32 = 500;

/// Deduplication thresholds. Two frames are the same shot when their difference
/// hashes are within `VIDEO_DHASH_THRESHOLD` bits (of 64) *and* every cell of a
/// 4x4 mean-colour grid is within `VIDEO_COLOR_THRESHOLD` (of 255).
///
/// Both conditions are required. Structure alone treats a cut between two flat
/// differently-coloured scenes as identical, because a difference hash reads
/// luminance gradients and a flat frame has none. Colour alone merges scenes
/// that share a palette.
pub const VIDEO_DHASH_THRESHOLD: u32 = 6;
pub const VIDEO_COLOR_THRESHOLD: u32 = 24;

impl ModerationModel {
    pub fn all_models() -> Vec<ModerationModel> {
        vec![
            ModerationModel::Auto,
            ModerationModel::Observer,
            ModerationModel::Sentinel,
            ModerationModel::Arbiter,
        ]
    }

    pub fn concrete_models() -> Vec<ModerationModel> {
        vec![
            ModerationModel::Observer,
            ModerationModel::Sentinel,
            ModerationModel::Arbiter,
        ]
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, ModerationModel::Auto)
    }

    pub fn version(&self) -> &str {
        match self {
            ModerationModel::Auto => "2.2",
            ModerationModel::Observer => "2.2",
            ModerationModel::Sentinel => "2.2",
            ModerationModel::Arbiter => "2.2",
        }
    }

    pub fn base_name(&self) -> &str {
        match self {
            ModerationModel::Auto => "Auto",
            ModerationModel::Observer => "Observer",
            ModerationModel::Sentinel => "Sentinel",
            ModerationModel::Arbiter => "Arbiter",
        }
    }

    pub fn to_name(&self) -> String {
        format!("{} {}", self.base_name(), self.version())
    }

    pub fn credits_per_byte(&self) -> i64 {
        match self {
            ModerationModel::Auto => {
                panic!("Auto must be resolved before calculating credits")
            }
            ModerationModel::Observer => 1,
            ModerationModel::Sentinel => 2,
            ModerationModel::Arbiter => 4,
        }
    }

    /// Resolve Auto to a concrete model based on remaining credits and allowed models.
    ///
    /// Uses weighted allocation: given n models sorted best-to-worst with weights n, n-1, ..., 1,
    /// the threshold to use model at rank r is: sum(1..r) / total_weight * max_credits.
    pub fn resolve_auto(
        remaining_credits: i64,
        max_credits: i64,
        allowed_models: &[ModerationModel],
    ) -> ModerationModel {
        let mut concrete: Vec<ModerationModel> = allowed_models
            .iter()
            .filter(|m| !m.is_auto())
            .cloned()
            .collect();

        if concrete.is_empty() {
            return ModerationModel::Observer;
        }

        // Sort by credits_per_byte descending (best/most expensive first)
        concrete.sort_by(|a, b| b.credits_per_byte().cmp(&a.credits_per_byte()));

        let n = concrete.len() as i64;
        let total_weight = n * (n + 1) / 2;

        // For each model at rank r (0-indexed), threshold = sum(1..=r) / total_weight * max_credits
        // sum(1..=r) for r=0 is 0, r=1 is 1, r=2 is 3, etc.
        let mut cumulative = 0i64;
        for (i, model) in concrete.iter().enumerate() {
            // cumulative = sum of weights of all models ranked worse (higher index)
            // threshold = cumulative / total_weight * max_credits
            let threshold = if max_credits > 0 {
                cumulative * max_credits / total_weight
            } else {
                0
            };

            if remaining_credits > threshold {
                return model.clone();
            }

            // Add weight of this rank: weight = n - i
            cumulative += n - i as i64;
        }

        // Fallback to cheapest
        concrete.last().cloned().unwrap_or(ModerationModel::Observer)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsernameCheckRequest {
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsernameCheckResponse {
    pub flagged: bool,
    pub score: f64,
}

/// Response shape of the moderation service's `GET /labels`: a `labels` object
/// mapping label key to its human-readable description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelsResponse {
    pub labels: std::collections::HashMap<String, String>,
}

impl ModerationLabel {
    pub fn to_name(&self) -> &str {
        match self {
            ModerationLabel::P => "Profanity",
            ModerationLabel::T => "Toxicity",
            ModerationLabel::H => "Harassment",
            ModerationLabel::HR => "Hate/Racism",
            ModerationLabel::I => "Insult",
            ModerationLabel::S => "Sexual",
            ModerationLabel::SU => "Sexual (Unlawful)",
            ModerationLabel::S2 => "Sexual (Explicit)",
            ModerationLabel::SE => "Sensitive Content",
            ModerationLabel::V => "Violence",
            ModerationLabel::SH => "Self-Harm",
            ModerationLabel::M => "Medical/Injury",
            ModerationLabel::SP => "Spam",
            ModerationLabel::PM => "Promotional",
            ModerationLabel::SI => "Scam/Incoherent",
            ModerationLabel::IL => "Illegal Activity",
        }
    }

    pub fn to_short_name(&self) -> &str {
        match self {
            ModerationLabel::P => "P",
            ModerationLabel::T => "T",
            ModerationLabel::H => "H",
            ModerationLabel::HR => "HR",
            ModerationLabel::I => "I",
            ModerationLabel::S => "S",
            ModerationLabel::SU => "SU",
            ModerationLabel::S2 => "S2",
            ModerationLabel::SE => "SE",
            ModerationLabel::V => "V",
            ModerationLabel::SH => "SH",
            ModerationLabel::M => "M",
            ModerationLabel::SP => "SP",
            ModerationLabel::PM => "PM",
            ModerationLabel::SI => "SI",
            ModerationLabel::IL => "IL",
        }
    }

    pub fn all_labels() -> Vec<ModerationLabel> {
        vec![
            ModerationLabel::P,
            ModerationLabel::T,
            ModerationLabel::H,
            ModerationLabel::HR,
            ModerationLabel::I,
            ModerationLabel::S,
            ModerationLabel::SU,
            ModerationLabel::S2,
            ModerationLabel::SE,
            ModerationLabel::V,
            ModerationLabel::SH,
            ModerationLabel::M,
            ModerationLabel::SP,
            ModerationLabel::PM,
            ModerationLabel::SI,
            ModerationLabel::IL,
        ]
    }
}

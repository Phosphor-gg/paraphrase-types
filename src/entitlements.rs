//! What a plan unlocks.
//!
//! An entitlement is a feature gate, not an allowance. How much audio someone
//! can process is credits, and both the credit figure and the entitlement list
//! come from Stripe product metadata rather than from a table here. That is
//! deliberate: a hardcoded per-tier list is a second source of truth that
//! drifts from what is actually being sold, and the drift is invisible until a
//! customer is denied something they paid for.
//!
//! So there is no `Tier::entitlements()`. `lookup_key` is the whole contract:
//! Stripe names a feature, this maps it to a variant, and the dashboard gates on
//! the result.

use serde::{Deserialize, Serialize};

/// A feature a plan can unlock.
///
/// The serialised name and the Stripe lookup key are deliberately the same
/// string, asserted by a test. Carrying two names for one feature is how a
/// dashboard ends up gating on a key Stripe has never heard of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Entitlement {
    /// The one-button cleanup: the model decides what to cut and reorder.
    ///
    /// Hand-editing is not gated. Someone on the free plan gets their
    /// transcript and can remove and reorder words themselves; what premium
    /// buys is having it done for them.
    AiCleanup,
    /// Export as WAV or FLAC rather than only the lossy formats.
    LosslessExport,
    /// Recordings longer than the free plan's per-recording cap.
    LongRecordings,
    /// Transcribing a file or a URL you already have, rather than recording.
    Transcription,
    /// API keys, for driving this from something other than the website.
    ApiAccess,
}

impl Entitlement {
    pub fn all() -> &'static [Entitlement] {
        &[
            Self::AiCleanup,
            Self::LosslessExport,
            Self::LongRecordings,
            Self::Transcription,
            Self::ApiAccess,
        ]
    }

    /// The feature key as it is named in Stripe.
    pub fn lookup_key(&self) -> &'static str {
        match self {
            Self::AiCleanup => "ai-cleanup",
            Self::LosslessExport => "lossless-export",
            Self::LongRecordings => "long-recordings",
            Self::Transcription => "transcription",
            Self::ApiAccess => "api-access",
        }
    }

    pub fn from_lookup_key(key: &str) -> Option<Self> {
        Self::all().iter().copied().find(|e| e.lookup_key() == key)
    }

    /// How the feature reads on a pricing page.
    pub fn label(&self) -> &'static str {
        match self {
            Self::AiCleanup => "Clean up recordings with one button",
            Self::LosslessExport => "Lossless WAV and FLAC export",
            Self::LongRecordings => "Long recordings",
            Self::Transcription => "Transcribe files and links",
            Self::ApiAccess => "API access",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lookup_key_round_trips() {
        for e in Entitlement::all() {
            assert_eq!(Entitlement::from_lookup_key(e.lookup_key()), Some(*e));
        }
    }

    #[test]
    fn an_unknown_key_is_none_rather_than_a_guess() {
        // Stripe can name a feature this build has never heard of, and
        // resolving it to the wrong variant would grant the wrong thing.
        assert_eq!(Entitlement::from_lookup_key("observer-model"), None);
        assert_eq!(Entitlement::from_lookup_key(""), None);
        assert_eq!(Entitlement::from_lookup_key("ai_cleanup"), None);
    }

    #[test]
    fn the_wire_name_and_the_stripe_key_are_the_same_string() {
        // Two names for one feature is how a dashboard gates on a key Stripe
        // has never heard of.
        for e in Entitlement::all() {
            let wire = serde_json::to_string(e).unwrap();
            assert_eq!(wire, format!("\"{}\"", e.lookup_key()), "{e:?}");
        }
    }

    #[test]
    fn no_two_features_share_a_key() {
        let mut keys: Vec<&str> = Entitlement::all().iter().map(|e| e.lookup_key()).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total);
    }

    #[test]
    fn every_feature_reads_as_a_benefit_not_a_variant_name() {
        for e in Entitlement::all() {
            let label = e.label();
            assert!(!label.is_empty());
            assert!(
                label.chars().next().unwrap().is_uppercase(),
                "{e:?} label should start a sentence"
            );
            assert!(!label.contains('-'), "{e:?} label looks like a key: {label}");
        }
    }

    #[test]
    fn an_entitlement_survives_a_round_trip() {
        for e in Entitlement::all() {
            let json = serde_json::to_string(e).unwrap();
            assert_eq!(serde_json::from_str::<Entitlement>(&json).unwrap(), *e);
        }
    }
}

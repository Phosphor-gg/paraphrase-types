//! The contract with `paraphrase-audio-api`, the GPU service.
//!
//! That service decodes audio into a word track and does nothing else. It does
//! not judge filler and it does not cut audio: the rules and the judge live in
//! the backend next to the edit model they produce, and the cut has to run in
//! the backend because the inference container mounts the media volume
//! read-only and only ever sees a 16kHz mono proxy of the master.
//!
//! [`TranscribeResponse`] denies unknown fields on purpose. The two sides of
//! this contract deploy together, so a field renamed in Python should fail the
//! request loudly rather than arrive as a silent `None` that empties every word
//! timing and makes the product look like it simply found no speech.

use serde::{Deserialize, Serialize};

use crate::edit::Word;

/// Decode one staged audio file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscribeRequest {
    /// Path within the shared media volume, as the GPU service sees it.
    pub audio_path: String,
    /// Force a language instead of detecting one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// The word track, and nothing that is not derived from the audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscribeResponse {
    /// Every word, in spoken order, with absolute offsets from the start of the
    /// file.
    ///
    /// Absolute rather than segment-relative because these are cut points: a
    /// caller that has to add a segment offset back on is a caller that will
    /// one day forget to.
    pub words: Vec<Word>,
    /// Detected or forced language, as a BCP-47 primary subtag.
    pub language: String,
    /// Length of the decoded audio.
    pub duration_ms: u32,
}

impl TranscribeResponse {
    /// Reject a response that cannot be cut against.
    ///
    /// A word track whose timings are absent, zero-length, out of order or past
    /// the end of the file produces a cut at the wrong place, and the failure is
    /// inaudible until someone listens to the deliverable. Checked on arrival
    /// rather than at render time, so the job fails where the cause is.
    pub fn validate(&self) -> Result<(), AsrError> {
        if self.words.is_empty() {
            return Err(AsrError::NoWords);
        }
        let mut prev_end = 0u32;
        for (position, w) in self.words.iter().enumerate() {
            if w.index as usize != position {
                return Err(AsrError::NonSequentialIndex { position, index: w.index });
            }
            if w.end_ms <= w.start_ms {
                return Err(AsrError::EmptySpan { index: w.index });
            }
            if w.start_ms < prev_end {
                return Err(AsrError::OverlappingSpan { index: w.index });
            }
            if w.end_ms > self.duration_ms {
                return Err(AsrError::SpanPastEnd {
                    index: w.index,
                    end_ms: w.end_ms,
                    duration_ms: self.duration_ms,
                });
            }
            prev_end = w.end_ms;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AsrError {
    /// Decoded, but produced no words.
    NoWords,
    /// Indices are not `0..n`, so an edit plan would address the wrong word.
    NonSequentialIndex { position: usize, index: u32 },
    /// A word with no duration cannot be cut.
    EmptySpan { index: u32 },
    /// Words whose audio overlaps cannot both be kept intact.
    OverlappingSpan { index: u32 },
    /// A timing past the end of the file; usually a chunk offset added twice.
    SpanPastEnd { index: u32, end_ms: u32, duration_ms: u32 },
}

impl std::fmt::Display for AsrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWords => write!(f, "the decoder returned no words"),
            Self::NonSequentialIndex { position, index } => {
                write!(f, "word at position {position} is numbered {index}")
            }
            Self::EmptySpan { index } => write!(f, "word {index} has no duration"),
            Self::OverlappingSpan { index } => {
                write!(f, "word {index} starts before the previous word ends")
            }
            Self::SpanPastEnd { index, end_ms, duration_ms } => write!(
                f,
                "word {index} ends at {end_ms}ms, past the {duration_ms}ms file"
            ),
        }
    }
}

impl std::error::Error for AsrError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::WordTrack;

    /// The committed contract. `paraphrase-audio-api` has a test asserting it
    /// produces this exact shape, so a rename on either side fails a test
    /// instead of emptying every word timing in production.
    const FIXTURE: &str = include_str!("../tests/fixtures/transcribe_response.json");

    fn ok_response() -> TranscribeResponse {
        TranscribeResponse {
            words: vec![
                Word::new(0, "um", 0, 200),
                Word::new(1, "hello", 250, 600),
                Word::new(2, "there", 620, 900),
            ],
            language: "en".into(),
            duration_ms: 1000,
        }
    }

    #[test]
    fn the_committed_fixture_parses() {
        let r: TranscribeResponse = serde_json::from_str(FIXTURE).unwrap();
        r.validate().unwrap();
        assert_eq!(r.language, "en");
        assert_eq!(r.words.len(), 6);
        assert_eq!(r.words[0].text, "um");
    }

    #[test]
    fn the_fixture_drops_straight_into_the_edit_model() {
        // The point of the contract: what the GPU service returns is already a
        // WordTrack, with no remapping step to get wrong.
        let r: TranscribeResponse = serde_json::from_str(FIXTURE).unwrap();
        let track = WordTrack::new(r.words);
        assert_eq!(track.len(), 6);
        assert!(crate::edit::is_hesitation(&track.words[0].text));
    }

    #[test]
    fn the_fixture_carries_per_word_confidence() {
        let r: TranscribeResponse = serde_json::from_str(FIXTURE).unwrap();
        assert!(
            r.words.iter().all(|w| w.confidence.is_some()),
            "the decoder gives confidence away free; the editor highlights with it"
        );
    }

    #[test]
    fn a_renamed_field_fails_loudly_rather_than_arriving_empty() {
        // This is the failure the contract exists to prevent: Python renames
        // `words` and the backend sees a transcript with no timings.
        let json = r#"{"word_list":[],"language":"en","duration_ms":1000}"#;
        assert!(serde_json::from_str::<TranscribeResponse>(json).is_err());
    }

    #[test]
    fn an_extra_field_fails_rather_than_being_ignored() {
        let json = r#"{"words":[],"language":"en","duration_ms":1000,"model":"turbo"}"#;
        assert!(serde_json::from_str::<TranscribeResponse>(json).is_err());
    }

    #[test]
    fn a_valid_response_validates() {
        ok_response().validate().unwrap();
    }

    #[test]
    fn a_response_with_no_words_is_refused() {
        let r = TranscribeResponse { words: vec![], language: "en".into(), duration_ms: 1000 };
        assert_eq!(r.validate(), Err(AsrError::NoWords));
    }

    #[test]
    fn renumbered_words_are_refused_because_plans_address_by_index() {
        let mut r = ok_response();
        r.words[1].index = 7;
        assert_eq!(
            r.validate(),
            Err(AsrError::NonSequentialIndex { position: 1, index: 7 })
        );
    }

    #[test]
    fn a_zero_length_word_is_refused() {
        let mut r = ok_response();
        r.words[1].end_ms = r.words[1].start_ms;
        assert_eq!(r.validate(), Err(AsrError::EmptySpan { index: 1 }));
    }

    #[test]
    fn overlapping_words_are_refused() {
        let mut r = ok_response();
        r.words[2].start_ms = 100; // before word 1 ends at 600
        assert_eq!(r.validate(), Err(AsrError::OverlappingSpan { index: 2 }));
    }

    #[test]
    fn a_word_past_the_end_of_the_file_is_refused() {
        // The shape of a chunk offset added twice, which is the real bug this
        // catches rather than a hypothetical one.
        let mut r = ok_response();
        r.words[2].end_ms = 5_000;
        assert_eq!(
            r.validate(),
            Err(AsrError::SpanPastEnd { index: 2, end_ms: 5_000, duration_ms: 1_000 })
        );
    }

    #[test]
    fn every_asr_error_says_which_word() {
        for e in [
            AsrError::NonSequentialIndex { position: 1, index: 7 },
            AsrError::EmptySpan { index: 3 },
            AsrError::OverlappingSpan { index: 4 },
            AsrError::SpanPastEnd { index: 5, end_ms: 10, duration_ms: 5 },
        ] {
            let m = e.to_string();
            assert!(m.contains("word"), "{m:?}");
        }
    }

    #[test]
    fn a_request_omits_language_when_detecting() {
        let r = TranscribeRequest { audio_path: "/media/a.wav".into(), language: None };
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("language"), "{json}");
    }
}

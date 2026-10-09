//! Recordings: a take inside a project, from raw audio to a cut deliverable.
//!
//! A recording owns one immutable master audio file and one word track derived
//! from it. Everything a person does afterwards is an [`EditPlan`] over that
//! track, which is why the master is never modified and never discarded: the
//! free re-edit is only possible while the original audio is still on disk.

use serde::{Deserialize, Serialize};

use crate::edit::{EditPlan, RejectedCut, Removal, Word};

/// Largest upload accepted, enforced on arriving bytes rather than on
/// `Content-Length`, which a client controls.
///
/// Generous because someone may upload a WAV master rather than a browser
/// recording: an hour of 48kHz stereo PCM is about 700 MB, while an hour of the
/// opus a browser produces is nearer 30 MB.
pub const MAX_UPLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Longest recording accepted. Beyond this the single-pass decode stops fitting
/// comfortably in GPU memory.
pub const MAX_RECORDING_SECS: u32 = 4 * 60 * 60;

/// Shortest recording worth analysing.
pub const MIN_RECORDING_MS: u32 = 500;

/// One credit buys one second of source audio.
///
/// There is one model and no multiplier, so this is an identity rather than a
/// rate: it exists to be named at call sites instead of a bare `1`, and to give
/// the minute helpers one place to disagree with.
pub const CREDITS_PER_MEDIA_SECOND: i64 = 1;

/// Credits charged for a recording of this length, rounding a part-second up.
///
/// Rounds up because a 1.2 second recording still occupies a GPU for the same
/// fixed overhead as a 2 second one, and because rounding down makes a
/// sub-second recording free.
pub fn credits_for_duration(duration_ms: u32) -> i64 {
    let seconds = (duration_ms as i64 + 999) / 1000;
    seconds * CREDITS_PER_MEDIA_SECOND
}

/// Render a millisecond duration the way the dashboard shows it.
pub fn format_duration(duration_ms: u32) -> String {
    let total = duration_ms / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 { format!("{h}h {m}m") } else if m > 0 { format!("{m}m {s}s") } else { format!("{s}s") }
}

/// Where a recording is in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingStatus {
    /// Bytes are still arriving.
    Uploading,
    /// Staged and waiting for a worker.
    Queued,
    /// Running the decode that produces the word track.
    Transcribing,
    /// Running the rules and the judge that propose the first edit.
    Marking,
    /// Word track and an edit plan are available.
    Ready,
    /// Gave up. The reason is on the recording.
    Failed,
}

impl RecordingStatus {
    /// Whether this status will ever change on its own.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Ready | Self::Failed)
    }

    /// Whether the word track and plan can be read.
    pub fn is_editable(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Why a recording failed, in terms a person can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingFailure {
    /// ffprobe could not read the container at all.
    Unreadable,
    /// The container carries no duration and the stream could not be measured.
    ///
    /// Its own variant because a browser `MediaRecorder` blob routinely omits
    /// duration, so this is an expected case with a specific remedy rather than
    /// a corrupt file.
    DurationUnknown,
    /// Longer than [`MAX_RECORDING_SECS`].
    TooLong,
    /// Shorter than [`MIN_RECORDING_MS`].
    TooShort,
    /// Decoded, but no words came out.
    NoSpeech,
    /// Not enough credits at submission.
    InsufficientCredits,
    /// The decode itself failed.
    TranscriptionFailed,
    /// Anything unexpected.
    Internal,
}

impl RecordingFailure {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Unreadable => "That file could not be read as audio.",
            Self::DurationUnknown => "The length of that recording could not be determined.",
            Self::TooLong => "That recording is longer than four hours.",
            Self::TooShort => "That recording is too short to work with.",
            Self::NoSpeech => "No speech was found in that recording.",
            Self::InsufficientCredits => "You do not have enough credits for that recording.",
            Self::TranscriptionFailed => "Transcribing that recording failed.",
            Self::Internal => "Something went wrong with that recording.",
        }
    }

    /// Whether trying the same file again could plausibly succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::TranscriptionFailed | Self::Internal)
    }
}

/// A recording as the dashboard lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingResponse {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub status: RecordingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<RecordingFailure>,
    /// Length of the original audio.
    pub duration_ms: u32,
    /// Length after the current edit, once there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited_duration_ms: Option<u32>,
    pub word_count: u32,
    pub removed_word_count: u32,
    pub created_at: String,
    pub updated_at: String,
}

/// A recording plus everything the editor needs to open it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingDetail {
    pub recording: RecordingResponse,
    pub words: Vec<Word>,
    /// Fingerprint of `words`. A plan saved against a different one is stale.
    pub words_sha256: String,
    pub plan: EditPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRecordingRequest {
    pub name: String,
}

/// Transcribe something that already exists somewhere else.
///
/// The secondary feature: the primary one is recording your own voice. The
/// server fetches the audio, so the URL is validated against pointing back at
/// private infrastructure before anything is fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateFromUrlRequest {
    pub url: String,
    /// Left out, the fetched media's own title is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Save a hand-made edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavePlanRequest {
    pub plan: EditPlan,
    /// The word track this plan was built against.
    ///
    /// Required, because a re-transcribe renumbers every word and silently
    /// applying an old plan to a new track cuts audio at the wrong places. A
    /// mismatch is refused rather than reconciled.
    pub words_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavePlanResponse {
    pub plan: EditPlan,
    pub edited_duration_ms: u32,
}

/// Run the automatic cleanup. Takes no options: the one button is the feature.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanRequest {
    /// Trim pauses longer than this as part of the same pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_gap_ms: Option<u32>,
}

/// What the cleanup did, itemised so the editor can highlight every change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanResponse {
    pub plan: EditPlan,
    pub removed: Vec<Removal>,
    /// Whether the edit plays any words out of their recorded order.
    ///
    /// Surfaced because a reorder is the one change worth previewing before
    /// rendering: pitch and pace do not match across a spliced join, so it is
    /// audibly worse than a deletion and a person should hear it first.
    pub reordered: bool,
    pub edited_duration_ms: u32,
    /// Cuts the model asked for that could not be placed, so they were not made.
    ///
    /// A dropped quote is a cut the person asked for and did not get, and it is
    /// the only signal that the reply was worse than it looks: the edit itself
    /// is structurally identical whether the model quoted well or badly.
    pub cuts_rejected: Vec<RejectedCut>,
    /// Windows left exactly as recorded because nothing usable came back.
    ///
    /// Travels with the response rather than staying in the log: the person is
    /// about to review this edit, and a passage that was never cleaned looks
    /// identical to one the model judged already clean.
    pub windows_failed: usize,
}

/// Audio formats a cut can be delivered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Wav,
    Mp3,
    M4a,
    Flac,
    Opus,
}

impl AudioFormat {
    pub fn all() -> &'static [AudioFormat] {
        &[Self::Wav, Self::Mp3, Self::M4a, Self::Flac, Self::Opus]
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Flac => "flac",
            Self::Opus => "opus",
        }
    }

    pub fn mime(&self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
            Self::M4a => "audio/mp4",
            Self::Flac => "audio/flac",
            Self::Opus => "audio/ogg",
        }
    }

    /// Whether the format keeps every sample it was given.
    pub fn is_lossless(&self) -> bool {
        matches!(self, Self::Wav | Self::Flac)
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::Mp3
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderStatus {
    Queued,
    Rendering,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderRequest {
    #[serde(default)]
    pub format: AudioFormat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderResponse {
    pub id: String,
    pub recording_id: String,
    pub status: RenderStatus,
    pub format: AudioFormat,
    /// Present once the render is ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteRecordingResponse {
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_second_costs_one_credit_per_second() {
        assert_eq!(credits_for_duration(1_000), 1);
        assert_eq!(credits_for_duration(60_000), 60);
        assert_eq!(credits_for_duration(3_600_000), 3_600);
    }

    #[test]
    fn a_part_second_rounds_up_so_nothing_is_free() {
        assert_eq!(credits_for_duration(1), 1);
        assert_eq!(credits_for_duration(1_001), 2);
        assert_eq!(credits_for_duration(1_999), 2);
    }

    #[test]
    fn an_empty_recording_costs_nothing() {
        assert_eq!(credits_for_duration(0), 0);
    }

    #[test]
    fn the_credit_unit_is_one_per_second_with_no_multiplier() {
        // There is one model. If this ever stops being 1, every minute helper
        // and every quoted price changes with it.
        assert_eq!(CREDITS_PER_MEDIA_SECOND, 1);
        assert_eq!(credits_for_duration(MAX_RECORDING_SECS * 1000), 14_400);
    }

    #[test]
    fn durations_read_the_way_a_person_says_them() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45_000), "45s");
        assert_eq!(format_duration(90_000), "1m 30s");
        assert_eq!(format_duration(3_600_000), "1h 0m");
        assert_eq!(format_duration(5_400_000), "1h 30m");
    }

    #[test]
    fn only_ready_and_failed_are_terminal() {
        for s in [RecordingStatus::Ready, RecordingStatus::Failed] {
            assert!(s.is_terminal(), "{s:?}");
        }
        for s in [
            RecordingStatus::Uploading,
            RecordingStatus::Queued,
            RecordingStatus::Transcribing,
            RecordingStatus::Marking,
        ] {
            assert!(!s.is_terminal(), "{s:?}");
        }
    }

    #[test]
    fn a_recording_is_only_editable_once_ready() {
        assert!(RecordingStatus::Ready.is_editable());
        assert!(!RecordingStatus::Failed.is_editable());
        assert!(!RecordingStatus::Marking.is_editable());
    }

    #[test]
    fn every_failure_explains_itself_without_naming_internals() {
        for f in [
            RecordingFailure::Unreadable,
            RecordingFailure::DurationUnknown,
            RecordingFailure::TooLong,
            RecordingFailure::TooShort,
            RecordingFailure::NoSpeech,
            RecordingFailure::InsufficientCredits,
            RecordingFailure::TranscriptionFailed,
            RecordingFailure::Internal,
        ] {
            let m = f.message();
            assert!(m.ends_with('.'), "{f:?}: {m:?}");
            for leak in ["ffprobe", "ffmpeg", "whisper", "panic", "unwrap", "sql"] {
                assert!(!m.to_lowercase().contains(leak), "{f:?} leaks {leak:?}");
            }
        }
    }

    #[test]
    fn only_transient_failures_are_worth_retrying() {
        assert!(RecordingFailure::Internal.is_retryable());
        assert!(RecordingFailure::TranscriptionFailed.is_retryable());
        // Retrying these with the same file gives the same answer.
        assert!(!RecordingFailure::TooLong.is_retryable());
        assert!(!RecordingFailure::Unreadable.is_retryable());
        assert!(!RecordingFailure::NoSpeech.is_retryable());
    }

    #[test]
    fn statuses_travel_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&RecordingStatus::Transcribing).unwrap(),
            r#""transcribing""#
        );
        assert_eq!(
            serde_json::from_str::<RecordingStatus>(r#""ready""#).unwrap(),
            RecordingStatus::Ready
        );
    }

    #[test]
    fn a_failure_travels_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&RecordingFailure::DurationUnknown).unwrap(),
            r#""duration_unknown""#
        );
    }

    #[test]
    fn every_audio_format_has_a_distinct_extension_and_mime() {
        let mut exts: Vec<&str> = AudioFormat::all().iter().map(|f| f.extension()).collect();
        let n = exts.len();
        exts.sort_unstable();
        exts.dedup();
        assert_eq!(exts.len(), n, "two formats share an extension");
        for f in AudioFormat::all() {
            assert!(f.mime().contains('/'), "{f:?}");
        }
    }

    #[test]
    fn a_format_travels_as_its_extension() {
        // The wire name and the file extension being the same string is what
        // lets a download URL be built from the format without a lookup.
        for f in AudioFormat::all() {
            let wire = serde_json::to_string(f).unwrap();
            assert_eq!(wire, format!("\"{}\"", f.extension()), "{f:?}");
        }
    }

    #[test]
    fn lossless_formats_are_the_ones_that_keep_every_sample() {
        assert!(AudioFormat::Wav.is_lossless());
        assert!(AudioFormat::Flac.is_lossless());
        assert!(!AudioFormat::Mp3.is_lossless());
        assert!(!AudioFormat::Opus.is_lossless());
    }

    #[test]
    fn the_default_delivery_format_is_widely_playable() {
        assert_eq!(AudioFormat::default(), AudioFormat::Mp3);
    }

    #[test]
    fn a_url_request_may_omit_the_name() {
        let r: CreateFromUrlRequest =
            serde_json::from_str(r#"{"url":"https://example.com/a.mp3"}"#).unwrap();
        assert_eq!(r.name, None);
        // And the name is omitted from the wire when absent.
        assert!(!serde_json::to_string(&r).unwrap().contains("name"));
    }

    #[test]
    fn a_url_request_requires_a_url() {
        assert!(serde_json::from_str::<CreateFromUrlRequest>(r#"{"name":"x"}"#).is_err());
    }
    #[test]
    fn a_clean_request_needs_no_fields() {
        let r: CleanRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(r.max_gap_ms, None);
    }

    #[test]
    fn a_clean_response_carries_what_the_cleanup_did_not_manage() {
        // Both of these were only in the log before, so a gutted recording and
        // an already-clean one reached the editor looking the same.
        let response = CleanResponse {
            plan: EditPlan { kept: vec![0, 1], max_gap_ms: None, pad_ms: 30 },
            removed: vec![],
            reordered: false,
            edited_duration_ms: 200,
            cuts_rejected: vec![crate::edit::RejectedCut {
                ordinal: 0,
                quote: "the".into(),
                error: crate::edit::CutError::Ambiguous { occurrences: 2 },
            }],
            windows_failed: 1,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("cuts_rejected") && json.contains("windows_failed"), "{json}");
        assert_eq!(serde_json::from_str::<CleanResponse>(&json).unwrap(), response);
    }

    #[test]
    fn a_render_request_defaults_its_format() {
        let r: RenderRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(r.format, AudioFormat::default());
    }

    #[test]
    fn saving_a_plan_requires_the_track_it_was_built_against() {
        // Without words_sha256 this must not deserialise: a plan with no
        // fingerprint is a plan that could be applied to the wrong track.
        let err = serde_json::from_str::<SavePlanRequest>(
            r#"{"plan":{"kept":[0],"max_gap_ms":null,"pad_ms":30}}"#,
        );
        assert!(err.is_err(), "words_sha256 must be mandatory");
    }

    #[test]
    fn a_save_plan_request_round_trips() {
        let req = SavePlanRequest {
            plan: EditPlan { kept: vec![0, 2], max_gap_ms: Some(200), pad_ms: 30 },
            words_sha256: "abc123".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(serde_json::from_str::<SavePlanRequest>(&json).unwrap(), req);
    }

    #[test]
    fn the_upload_cap_admits_an_hour_of_uncompressed_stereo() {
        // 48kHz, 2 channels, 16-bit: the realistic worst case for a master.
        let hour_of_pcm: u64 = 48_000 * 2 * 2 * 3_600;
        assert!(
            MAX_UPLOAD_BYTES > hour_of_pcm,
            "cap {MAX_UPLOAD_BYTES} rejects a {hour_of_pcm}-byte WAV"
        );
    }
}

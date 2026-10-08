//! The edit model: which transcribed words survive, and in what order.
//!
//! Everything a Paraphrase edit can express is a statement about *original*
//! words. A plan is an ordered list of indices into the transcript, so a word
//! that was never spoken is not something the prompt forbids, it is something
//! this representation cannot encode. That is deliberate: the output audio is
//! the original recording with pieces removed, and there is no audio for a word
//! nobody said.
//!
//! The same plan serves a manual edit and an AI edit. Removing a word, undoing
//! that, reordering, and closing a long pause are all edits to one structure,
//! which is why the "clean it up for me" button and hand-editing cannot drift
//! apart.

use serde::{Deserialize, Serialize};

/// Default padding kept either side of a word so a cut does not clip its edges.
pub const DEFAULT_PAD_MS: u32 = 30;

/// Longest repeated run `propose_fillers` will treat as a false start.
const MAX_REPEATED_RUN: usize = 6;

/// A transcribed word and the span of audio that produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    /// Position in the original transcript. Stable; it is how edits refer to
    /// this word for the life of the recording.
    pub index: u32,
    pub text: String,
    pub start_ms: u32,
    pub end_ms: u32,
    /// How sure the decoder was, when it says. Drives the editor's highlighting
    /// of words worth re-listening to; never used to decide a cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

impl Word {
    pub fn new(index: u32, text: impl Into<String>, start_ms: u32, end_ms: u32) -> Self {
        Self { index, text: text.into(), start_ms, end_ms, confidence: None }
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn duration_ms(&self) -> u32 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// The words of one recording, in spoken order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WordTrack {
    pub words: Vec<Word>,
}

impl WordTrack {
    /// Build a track, renumbering indices so they are always `0..len`.
    ///
    /// Indices are positional by construction rather than by trust, because
    /// every edit is an index and an off-by-one would cut the wrong audio.
    pub fn new(words: impl IntoIterator<Item = Word>) -> Self {
        let words = words
            .into_iter()
            .enumerate()
            .map(|(i, mut w)| {
                w.index = i as u32;
                w
            })
            .collect();
        Self { words }
    }

    /// Convenience for tests and for transcripts that carry no timings yet:
    /// lays words out end to end at a fixed duration each.
    pub fn from_text(text: &str, ms_per_word: u32) -> Self {
        let words = text
            .split_whitespace()
            .enumerate()
            .map(|(i, t)| {
                let start = i as u32 * ms_per_word;
                Word::new(i as u32, t, start, start + ms_per_word)
            })
            .collect();
        Self { words }
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn get(&self, index: u32) -> Option<&Word> {
        self.words.get(index as usize)
    }

    /// The full transcript as spoken.
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    /// Total speech span, ignoring leading silence.
    pub fn duration_ms(&self) -> u32 {
        self.words.last().map(|w| w.end_ms).unwrap_or(0)
    }
}

/// Why a word was dropped. Carried for the UI, never for correctness: the audio
/// cut depends only on which indices survive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalReason {
    /// A hesitation sound: um, uh, erm.
    Hesitation,
    /// An immediately repeated word or phrase, or a restarted sentence.
    Repetition,
    /// A discourse filler that carried no meaning here.
    Filler,
    /// The model judged this not worth keeping for another reason.
    Model,
    /// The person removed it by hand.
    Manual,
}

/// An ordered selection of original words, plus how to treat the pauses between
/// them.
///
/// `kept` is the playback order, so reordering is expressible, and an index
/// absent from `kept` is a removed word. The unedited plan is every index in
/// order, which is why "revert to original" needs no stored copy of anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditPlan {
    /// Original word indices to keep, in playback order.
    pub kept: Vec<u32>,
    /// Trim any pause between two kept, originally-adjacent words down to this
    /// many milliseconds. `None` leaves pauses as recorded.
    pub max_gap_ms: Option<u32>,
    /// Audio kept either side of each word so cuts do not clip consonants.
    pub pad_ms: u32,
}

impl EditPlan {
    /// The plan that changes nothing.
    pub fn unedited(track: &WordTrack) -> Self {
        Self {
            kept: (0..track.len() as u32).collect(),
            max_gap_ms: None,
            pad_ms: DEFAULT_PAD_MS,
        }
    }

    pub fn is_unedited(&self, track: &WordTrack) -> bool {
        self.max_gap_ms.is_none() && self.kept.len() == track.len()
            && self.kept.iter().enumerate().all(|(i, &k)| k == i as u32)
    }

    /// Indices present in the track but absent from the plan.
    pub fn removed(&self, track: &WordTrack) -> Vec<u32> {
        let kept: std::collections::HashSet<u32> = self.kept.iter().copied().collect();
        (0..track.len() as u32).filter(|i| !kept.contains(i)).collect()
    }

    pub fn remove(&mut self, index: u32) {
        self.kept.retain(|&k| k != index);
    }

    /// Put a removed word back at its original position relative to the words
    /// still kept. Does nothing if it is already kept.
    pub fn restore(&mut self, index: u32) {
        if self.kept.contains(&index) {
            return;
        }
        let at = self.kept.partition_point(|&k| k < index);
        self.kept.insert(at, index);
    }

    /// Move the word at playback position `from` to position `to`.
    pub fn move_word(&mut self, from: usize, to: usize) -> Result<(), EditError> {
        if from >= self.kept.len() || to >= self.kept.len() {
            return Err(EditError::PositionOutOfRange {
                position: from.max(to),
                len: self.kept.len(),
            });
        }
        let w = self.kept.remove(from);
        self.kept.insert(to, w);
        Ok(())
    }

    /// The edited transcript.
    pub fn text(&self, track: &WordTrack) -> String {
        self.kept
            .iter()
            .filter_map(|&i| track.get(i))
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Reject a plan that does not refer to this track's words exactly once
    /// each.
    ///
    /// Invention cannot reach here, because a plan holds indices. What this
    /// catches is a stale plan against a re-transcribed recording, and a word
    /// used twice (which would duplicate its audio).
    pub fn validate(&self, track: &WordTrack) -> Result<(), EditError> {
        let mut seen = vec![false; track.len()];
        for &i in &self.kept {
            let slot = seen.get_mut(i as usize).ok_or(EditError::IndexOutOfRange {
                index: i,
                len: track.len(),
            })?;
            if *slot {
                return Err(EditError::DuplicateWord { index: i });
            }
            *slot = true;
        }
        Ok(())
    }

    /// The spans of original audio to keep, in playback order.
    ///
    /// Words that were adjacent in the recording and stay adjacent in playback
    /// are merged into one span, so continuous speech is never needlessly cut
    /// and rejoined. Padding is clamped to half the silence either side, which
    /// is what stops a 30ms pad from leaving the tail of a removed "um"
    /// audible.
    pub fn keep_intervals(&self, track: &WordTrack) -> Vec<Interval> {
        let mut out: Vec<Interval> = Vec::new();
        let mut prev_index: Option<u32> = None;

        for &i in &self.kept {
            let Some(w) = track.get(i) else { continue };

            let prev_word = i.checked_sub(1).and_then(|p| track.get(p));
            let next_word = track.get(i + 1);

            // Half the silence either side, so a pad can never reach into a
            // neighbouring word and leave the tail of a removed "um" audible.
            // At the edges of the recording there is no neighbour to protect,
            // so the only limit is the file itself.
            let left = match prev_word {
                Some(p) => self.pad_ms.min(w.start_ms.saturating_sub(p.end_ms) / 2),
                None => self.pad_ms.min(w.start_ms),
            };
            let right = match next_word {
                Some(n) => self.pad_ms.min(n.start_ms.saturating_sub(w.end_ms) / 2),
                None => self.pad_ms,
            };
            let start = w.start_ms.saturating_sub(left);
            let end = w.end_ms + right;

            // Contiguous in the recording and still contiguous in playback, so
            // this is continuous speech: extend the open span rather than
            // cutting and rejoining it.
            if prev_index == i.checked_sub(1) && !out.is_empty() {
                let prev_end = prev_word.map(|p| p.end_ms).unwrap_or(w.start_ms);
                let pause = w.start_ms.saturating_sub(prev_end);
                match self.max_gap_ms {
                    Some(max) if pause > max => {
                        // Keep only `max` of the pause, split across the join so
                        // neither word starts abruptly.
                        let head = max / 2;
                        out.last_mut().expect("checked non-empty").end_ms = prev_end + head;
                        out.push(Interval {
                            start_ms: w.start_ms.saturating_sub(max - head),
                            end_ms: end,
                        });
                    }
                    _ => out.last_mut().expect("checked non-empty").end_ms = end,
                }
                prev_index = Some(i);
                continue;
            }

            out.push(Interval { start_ms: start, end_ms: end });
            prev_index = Some(i);
        }
        out
    }

    /// How long the edited audio will be.
    pub fn output_duration_ms(&self, track: &WordTrack) -> u32 {
        self.keep_intervals(track).iter().map(|i| i.duration_ms()).sum()
    }
}

/// A span of the original audio, in original-recording time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interval {
    pub start_ms: u32,
    pub end_ms: u32,
}

impl Interval {
    pub fn duration_ms(&self) -> u32 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EditError {
    IndexOutOfRange { index: u32, len: usize },
    DuplicateWord { index: u32 },
    PositionOutOfRange { position: usize, len: usize },
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IndexOutOfRange { index, len } => {
                write!(f, "word {index} is outside this recording's {len} words")
            }
            Self::DuplicateWord { index } => {
                write!(f, "word {index} is kept more than once")
            }
            Self::PositionOutOfRange { position, len } => {
                write!(f, "position {position} is outside the {len} kept words")
            }
        }
    }
}

impl std::error::Error for EditError {}

/// Lowercase and strip everything but alphanumerics, so a word matches across
/// re-punctuation: `"Um,"` and `"um"`, `"don't"` and `"dont"`.
pub fn normalize(word: &str) -> String {
    word.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Sounds that are never content.
const HESITATIONS: &[&str] = &[
    "um", "umm", "ummm", "uh", "uhh", "uhhh", "er", "err", "erm", "ermm", "ah", "ahh", "eh",
    "hmm", "hm", "mmm", "mm", "mhm", "uhhuh", "huh",
];

pub fn is_hesitation(word: &str) -> bool {
    HESITATIONS.contains(&normalize(word).as_str())
}

/// A removal the AI or a rule proposed, with the reason for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removal {
    pub index: u32,
    pub reason: RemovalReason,
}

/// Deterministic cleanup: hesitation sounds, immediately repeated words, and
/// restarted phrases.
///
/// This is the floor the product stands on with no model running at all, and it
/// is what the LLM's output is diffed against when judging whether the model
/// actually helped.
pub fn propose_fillers(track: &WordTrack) -> Vec<Removal> {
    let mut removals: Vec<Removal> = Vec::new();

    // Hesitations first, so repetition detection sees "I I" in "I um I".
    let mut surviving: Vec<u32> = Vec::with_capacity(track.len());
    for w in &track.words {
        if is_hesitation(&w.text) {
            removals.push(Removal { index: w.index, reason: RemovalReason::Hesitation });
        } else {
            surviving.push(w.index);
        }
    }

    // Then immediately repeated runs: "the the" and "I went to the I went to
    // the store" both drop the earlier copy, keeping the completed attempt.
    let norm: Vec<String> =
        surviving.iter().filter_map(|&i| track.get(i)).map(|w| normalize(&w.text)).collect();

    let mut i = 0usize;
    while i < surviving.len() {
        let mut matched = 0usize;
        let max_k = MAX_REPEATED_RUN.min((surviving.len() - i) / 2);
        for k in (1..=max_k).rev() {
            if norm[i..i + k] == norm[i + k..i + 2 * k] && norm[i..i + k].iter().all(|w| !w.is_empty())
            {
                matched = k;
                break;
            }
        }
        if matched > 0 {
            for &idx in &surviving[i..i + matched] {
                removals.push(Removal { index: idx, reason: RemovalReason::Repetition });
            }
            i += matched;
        } else {
            i += 1;
        }
    }

    removals.sort_by_key(|r| r.index);
    removals.dedup_by_key(|r| r.index);
    removals
}

/// Apply removals to a plan.
pub fn apply_removals(plan: &mut EditPlan, removals: &[Removal]) {
    let drop: std::collections::HashSet<u32> = removals.iter().map(|r| r.index).collect();
    plan.kept.retain(|k| !drop.contains(k));
}

// ---------------------------------------------------------------------------
// Turning a model's answer into a plan
// ---------------------------------------------------------------------------

/// One deletion the model asked for: the index, plus the word it believes is
/// there.
///
/// The echoed word is the whole point. A model that miscounts its way down a
/// numbered list produces indices that are confidently wrong, and an index
/// alone is unfalsifiable. Echoing the word makes an off-by-N self-evident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedDeletion {
    pub index: u32,
    pub word: String,
    #[serde(default)]
    pub reason: Option<RemovalReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletionProposal {
    pub deletions: Vec<ProposedDeletion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProposalError {
    /// The index is not a word in this recording.
    IndexOutOfRange { index: u32, len: usize },
    /// The model's own echo disagrees with the word at that index, so its
    /// counting has drifted and none of its indices can be trusted.
    WordMismatch { index: u32, expected: String, got: String },
    /// Everything was deleted. Always a bug, never an edit worth rendering.
    DeletesEverything,
}

impl std::fmt::Display for ProposalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IndexOutOfRange { index, len } => {
                write!(f, "word {index} is outside this recording's {len} words")
            }
            Self::WordMismatch { index, expected, got } => write!(
                f,
                "word {index} is {expected:?} but the model called it {got:?}"
            ),
            Self::DeletesEverything => write!(f, "the proposal removes every word"),
        }
    }
}

impl std::error::Error for ProposalError {}

impl DeletionProposal {
    /// Check every deletion against the track and turn the proposal into
    /// removals.
    ///
    /// Rejects the whole proposal on the first disagreement rather than
    /// dropping the bad entry: a model whose indices have drifted is wrong
    /// everywhere after the drift, and partially applying that cuts audio at
    /// random.
    pub fn validate(&self, track: &WordTrack) -> Result<Vec<Removal>, ProposalError> {
        let mut removals = Vec::with_capacity(self.deletions.len());
        for d in &self.deletions {
            let word = track.get(d.index).ok_or(ProposalError::IndexOutOfRange {
                index: d.index,
                len: track.len(),
            })?;
            if normalize(&word.text) != normalize(&d.word) {
                return Err(ProposalError::WordMismatch {
                    index: d.index,
                    expected: word.text.clone(),
                    got: d.word.clone(),
                });
            }
            removals.push(Removal {
                index: d.index,
                reason: d.reason.unwrap_or(RemovalReason::Model),
            });
        }
        removals.sort_by_key(|r| r.index);
        removals.dedup_by_key(|r| r.index);

        if !track.is_empty() && removals.len() == track.len() {
            return Err(ProposalError::DeletesEverything);
        }
        Ok(removals)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AlignError {
    /// A word in the cleaned text is not available in the remaining original
    /// words. Either the model invented it or it reordered, and neither can be
    /// turned into an audio cut.
    UnmatchedWord { word: String, after_index: usize },
}

impl std::fmt::Display for AlignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnmatchedWord { word, after_index } => write!(
                f,
                "{word:?} does not occur in the recording after word {after_index}; \
                 the model invented or moved it"
            ),
        }
    }
}

impl std::error::Error for AlignError {}

/// Align cleaned free text back onto the original words, keeping only a
/// subsequence.
///
/// This exists for models that will not reliably emit indices, and it is strict
/// on purpose: an output word with no match left in the original is an error,
/// not a deletion. Treating it as a deletion is how a rephrase silently becomes
/// a cut of real speech.
pub fn plan_from_cleaned_text(track: &WordTrack, cleaned: &str) -> Result<Vec<u32>, AlignError> {
    let mut kept = Vec::new();
    let mut cursor = 0usize;

    for token in cleaned.split_whitespace() {
        let want = normalize(token);
        if want.is_empty() {
            continue;
        }
        let found = (cursor..track.len()).find(|&j| normalize(&track.words[j].text) == want);
        match found {
            Some(j) => {
                kept.push(track.words[j].index);
                cursor = j + 1;
            }
            None => {
                return Err(AlignError::UnmatchedWord {
                    word: token.to_string(),
                    after_index: cursor,
                })
            }
        }
    }
    Ok(kept)
}

/// Render the numbered word list a model is asked to choose deletions from.
///
/// Every word carries its own index so the model copies a number instead of
/// counting to one, which is the difference between an occasional off-by-one and
/// a systematic drift.
pub fn numbered_words(track: &WordTrack) -> String {
    track
        .words
        .iter()
        .map(|w| format!("{}:{}", w.index, w.text))
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Candidates: what the rules suspect but cannot decide alone
// ---------------------------------------------------------------------------

/// Discourse markers whose filler-ness depends entirely on context.
///
/// "it tastes *like* chicken" is a comparison and "by *like* Friday" is a tic,
/// and no rule distinguishes them. These are proposed, never removed outright.
const AMBIGUOUS_WORDS: &[&str] = &[
    "like", "basically", "actually", "literally", "really", "just", "so", "well",
    "right", "okay", "anyway", "obviously", "honestly", "totally", "essentially",
];

/// Multi-word markers, matched over surviving words so a hesitation between the
/// halves does not hide them.
const AMBIGUOUS_PHRASES: &[&[&str]] = &[
    &["you", "know"],
    &["i", "mean"],
    &["sort", "of"],
    &["kind", "of"],
    &["you", "see"],
    &["or", "something"],
    &["or", "whatever"],
];

/// A span the rules suspect is filler but will not remove without a judgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Original word indices, ascending.
    pub indices: Vec<u32>,
    /// The words themselves, for the prompt and for the UI.
    pub text: String,
}

/// A judgement on one candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Removing it leaves the meaning identical.
    Filler,
    /// It carries meaning, or the sentence breaks without it.
    Content,
}

/// What a judge is asked about one candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Judgement {
    pub candidate: Candidate,
    pub verdict: Verdict,
}

/// Spans worth asking about, skipping anything already removed.
///
/// The indices come from here rather than from a model, which is what makes
/// index drift unrepresentable: a judge only ever answers "filler" or "content"
/// about a span this function chose.
pub fn candidate_spans(track: &WordTrack, removed: &[u32]) -> Vec<Candidate> {
    let gone: std::collections::HashSet<u32> = removed.iter().copied().collect();
    let surviving: Vec<u32> =
        (0..track.len() as u32).filter(|i| !gone.contains(i)).collect();
    let norm: Vec<String> = surviving
        .iter()
        .filter_map(|&i| track.get(i))
        .map(|w| normalize(&w.text))
        .collect();

    let mut out = Vec::new();
    let mut at = 0usize;
    while at < surviving.len() {
        // Longest phrase first, so "kind of" is not proposed as a bare "kind".
        let phrase = AMBIGUOUS_PHRASES.iter().find(|p| {
            at + p.len() <= surviving.len()
                && norm[at..at + p.len()].iter().zip(p.iter()).all(|(a, b)| a == b)
        });
        if let Some(p) = phrase {
            let indices: Vec<u32> = surviving[at..at + p.len()].to_vec();
            out.push(Candidate { text: words_text(track, &indices), indices });
            at += p.len();
            continue;
        }
        if AMBIGUOUS_WORDS.contains(&norm[at].as_str()) {
            let indices = vec![surviving[at]];
            out.push(Candidate { text: words_text(track, &indices), indices });
        }
        at += 1;
    }
    out
}

fn words_text(track: &WordTrack, indices: &[u32]) -> String {
    indices
        .iter()
        .filter_map(|&i| track.get(i))
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The sentence as it currently stands, with the candidate marked.
///
/// Words already removed are hidden, so the judge reads the text the listener
/// would actually hear rather than the raw transcript.
pub fn judge_context(track: &WordTrack, removed: &[u32], candidate: &Candidate) -> String {
    let gone: std::collections::HashSet<u32> = removed.iter().copied().collect();
    let span: std::collections::HashSet<u32> = candidate.indices.iter().copied().collect();
    let first = candidate.indices.first().copied();

    let mut parts: Vec<String> = Vec::new();
    for w in &track.words {
        if Some(w.index) == first {
            parts.push(format!("<<{}>>", candidate.text));
        } else if span.contains(&w.index) || gone.contains(&w.index) {
            continue;
        } else {
            parts.push(w.text.clone());
        }
    }
    parts.join(" ")
}

/// Turn judgements into removals, keeping only the filler verdicts.
pub fn removals_from_judgements(judged: &[Judgement]) -> Vec<Removal> {
    let mut out: Vec<Removal> = judged
        .iter()
        .filter(|j| j.verdict == Verdict::Filler)
        .flat_map(|j| {
            j.candidate
                .indices
                .iter()
                .map(|&index| Removal { index, reason: RemovalReason::Filler })
        })
        .collect();
    out.sort_by_key(|r| r.index);
    out.dedup_by_key(|r| r.index);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(text: &str) -> WordTrack {
        WordTrack::from_text(text, 100)
    }

    // -- the load-bearing invariant -----------------------------------------

    #[test]
    fn edited_text_never_contains_a_word_that_was_not_spoken() {
        let t = track("um so I basically went to the uh the store yesterday");
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &propose_fillers(&t));

        let spoken: Vec<String> = t.words.iter().map(|w| normalize(&w.text)).collect();
        for word in plan.text(&t).split_whitespace() {
            assert!(
                spoken.contains(&normalize(word)),
                "{word:?} is not in the recording"
            );
        }
    }

    #[test]
    fn every_kept_word_is_backed_by_exactly_one_original_word() {
        let t = track("one two three four five");
        let mut plan = EditPlan::unedited(&t);
        plan.remove(2);
        plan.validate(&t).unwrap();

        // Audio can only be reused once, so a duplicate is a hard error.
        plan.kept.push(1);
        assert_eq!(plan.validate(&t), Err(EditError::DuplicateWord { index: 1 }));
    }

    #[test]
    fn a_plan_cannot_reference_a_word_outside_the_recording() {
        let t = track("one two three");
        let plan = EditPlan { kept: vec![0, 7], max_gap_ms: None, pad_ms: 0 };
        assert_eq!(
            plan.validate(&t),
            Err(EditError::IndexOutOfRange { index: 7, len: 3 })
        );
    }

    // -- deterministic cleanup ----------------------------------------------

    #[test]
    fn hesitations_are_removed() {
        let t = track("um I uh went er to the store");
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &propose_fillers(&t));
        assert_eq!(plan.text(&t), "I went to the store");
    }

    #[test]
    fn hesitations_are_matched_despite_punctuation_and_case() {
        let t = track("Um, I went. Uh... to the store");
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &propose_fillers(&t));
        assert_eq!(plan.text(&t), "I went. to the store");
    }

    #[test]
    fn an_immediately_repeated_word_keeps_one_copy() {
        let t = track("I went to the the store");
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &propose_fillers(&t));
        assert_eq!(plan.text(&t), "I went to the store");
    }

    #[test]
    fn a_restarted_phrase_keeps_the_completed_attempt() {
        let t = track("I went to the I went to the store");
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &propose_fillers(&t));
        assert_eq!(plan.text(&t), "I went to the store");
        // It kept the second attempt, not the abandoned one.
        assert_eq!(plan.kept, vec![4, 5, 6, 7, 8]);
    }

    #[test]
    fn a_hesitation_between_two_copies_does_not_hide_the_repetition() {
        let t = track("I I um went to the store");
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &propose_fillers(&t));
        assert_eq!(plan.text(&t), "I went to the store");
    }

    #[test]
    fn clean_speech_is_left_alone() {
        let t = track("The quick brown fox jumps over the lazy dog");
        let removals = propose_fillers(&t);
        assert!(removals.is_empty(), "{removals:?}");
    }

    #[test]
    fn a_legitimately_repeated_word_across_a_phrase_is_not_a_stutter() {
        // "the" twice, but not adjacent, so nothing is a repetition.
        let t = track("the cat sat on the mat");
        assert!(propose_fillers(&t).is_empty());
    }

    #[test]
    fn cleanup_is_idempotent() {
        let t = track("um I I went to the uh store");
        let mut once = EditPlan::unedited(&t);
        apply_removals(&mut once, &propose_fillers(&t));

        // Re-running over the surviving words must change nothing further.
        let survivors = WordTrack::new(
            once.kept.iter().filter_map(|&i| t.get(i)).cloned(),
        );
        assert!(
            propose_fillers(&survivors).is_empty(),
            "second pass still wanted to cut {:?}",
            propose_fillers(&survivors)
        );
    }

    #[test]
    fn content_words_numbers_and_names_survive() {
        let t = track("um so Jaiden spent 42 pounds uh on the thing");
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &propose_fillers(&t));
        let out = plan.text(&t);
        for must in ["Jaiden", "42", "pounds"] {
            assert!(out.contains(must), "{must:?} was lost from {out:?}");
        }
    }

    #[test]
    fn an_empty_recording_is_handled() {
        let t = track("");
        assert!(propose_fillers(&t).is_empty());
        let plan = EditPlan::unedited(&t);
        assert_eq!(plan.text(&t), "");
        assert_eq!(plan.keep_intervals(&t), vec![]);
        assert!(plan.is_unedited(&t));
    }

    // -- model proposals ----------------------------------------------------

    #[test]
    fn a_proposal_whose_words_agree_is_accepted() {
        let t = track("um I went to the store");
        let proposal = DeletionProposal {
            deletions: vec![ProposedDeletion {
                index: 0,
                word: "um".into(),
                reason: Some(RemovalReason::Hesitation),
            }],
        };
        let removals = proposal.validate(&t).unwrap();
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &removals);
        assert_eq!(plan.text(&t), "I went to the store");
    }

    #[test]
    fn a_drifted_index_is_rejected_rather_than_cutting_the_wrong_word() {
        let t = track("um I went to the store");
        // The model meant "um" but counted from one.
        let proposal = DeletionProposal {
            deletions: vec![ProposedDeletion { index: 1, word: "um".into(), reason: None }],
        };
        assert_eq!(
            proposal.validate(&t),
            Err(ProposalError::WordMismatch {
                index: 1,
                expected: "I".into(),
                got: "um".into()
            })
        );
    }

    #[test]
    fn one_bad_entry_rejects_the_whole_proposal() {
        let t = track("um I went to the store");
        let proposal = DeletionProposal {
            deletions: vec![
                ProposedDeletion { index: 0, word: "um".into(), reason: None },
                ProposedDeletion { index: 3, word: "the".into(), reason: None },
            ],
        };
        // Index 3 is "to", not "the". Nothing is applied, because after a drift
        // the earlier indices cannot be trusted either.
        assert!(proposal.validate(&t).is_err());
    }

    #[test]
    fn a_proposal_cannot_invent_a_word() {
        let t = track("I went to the store");
        // There is no index that holds a word nobody said, so the only way to
        // express this is an out-of-range index.
        let proposal = DeletionProposal {
            deletions: vec![ProposedDeletion { index: 99, word: "furthermore".into(), reason: None }],
        };
        assert_eq!(
            proposal.validate(&t),
            Err(ProposalError::IndexOutOfRange { index: 99, len: 5 })
        );
    }

    #[test]
    fn deleting_everything_is_rejected() {
        let t = track("um uh er");
        let proposal = DeletionProposal {
            deletions: vec![
                ProposedDeletion { index: 0, word: "um".into(), reason: None },
                ProposedDeletion { index: 1, word: "uh".into(), reason: None },
                ProposedDeletion { index: 2, word: "er".into(), reason: None },
            ],
        };
        assert_eq!(proposal.validate(&t), Err(ProposalError::DeletesEverything));
    }

    #[test]
    fn repeated_deletions_of_the_same_word_collapse() {
        let t = track("um I went");
        let proposal = DeletionProposal {
            deletions: vec![
                ProposedDeletion { index: 0, word: "um".into(), reason: None },
                ProposedDeletion { index: 0, word: "um".into(), reason: None },
            ],
        };
        assert_eq!(proposal.validate(&t).unwrap().len(), 1);
    }

    #[test]
    fn numbered_words_pairs_every_word_with_its_index() {
        let t = track("um I went");
        assert_eq!(numbered_words(&t), "0:um 1:I 2:went");
    }

    // -- free-text alignment ------------------------------------------------

    #[test]
    fn cleaned_text_that_only_deletes_aligns() {
        let t = track("um so I went to the store");
        let kept = plan_from_cleaned_text(&t, "I went to the store").unwrap();
        assert_eq!(kept, vec![2, 3, 4, 5, 6]);
    }

    #[test]
    fn alignment_tolerates_the_model_repunctuating() {
        let t = track("um I went to the store");
        let kept = plan_from_cleaned_text(&t, "I went to the store.").unwrap();
        assert_eq!(kept, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn an_invented_word_is_an_error_not_a_deletion() {
        let t = track("um I went to the store");
        // This is the failure that made the earlier prototype unusable: the
        // model rewrote rather than cut, and the rewrite was charged to the
        // audio as a deletion.
        let err = plan_from_cleaned_text(&t, "I subsequently went to the store").unwrap_err();
        assert!(matches!(err, AlignError::UnmatchedWord { ref word, .. } if word == "subsequently"));
    }

    #[test]
    fn a_wholesale_rephrase_is_rejected() {
        let t = track("um so I kind of went to the store yesterday");
        assert!(plan_from_cleaned_text(&t, "I visited the shop").is_err());
    }

    #[test]
    fn reordering_is_rejected_on_the_text_path() {
        let t = track("I went to the store");
        // Expressible by hand via move_word, but not inferable from free text.
        assert!(plan_from_cleaned_text(&t, "to the store I went").is_err());
    }

    #[test]
    fn aligned_text_is_always_a_subsequence() {
        let t = track("um so I basically went to the uh store");
        let kept = plan_from_cleaned_text(&t, "so I went to the store").unwrap();
        assert!(kept.windows(2).all(|w| w[0] < w[1]), "{kept:?} is not ascending");
    }

    // -- manual editing -----------------------------------------------------

    #[test]
    fn removing_and_restoring_returns_the_original() {
        let t = track("one two three four");
        let mut plan = EditPlan::unedited(&t);
        plan.remove(2);
        assert_eq!(plan.text(&t), "one two four");
        plan.restore(2);
        assert_eq!(plan.text(&t), "one two three four");
        assert!(plan.is_unedited(&t));
    }

    #[test]
    fn restore_puts_a_word_back_in_its_original_position() {
        let t = track("one two three four");
        let mut plan = EditPlan::unedited(&t);
        plan.remove(1);
        plan.remove(2);
        plan.restore(2);
        assert_eq!(plan.kept, vec![0, 2, 3]);
    }

    #[test]
    fn restoring_a_kept_word_is_a_no_op() {
        let t = track("one two three");
        let mut plan = EditPlan::unedited(&t);
        plan.restore(1);
        assert_eq!(plan.kept, vec![0, 1, 2]);
    }

    #[test]
    fn words_can_be_reordered_by_hand() {
        let t = track("world hello");
        let mut plan = EditPlan::unedited(&t);
        plan.move_word(1, 0).unwrap();
        assert_eq!(plan.text(&t), "hello world");
        plan.validate(&t).unwrap();
    }

    #[test]
    fn reordering_still_cannot_introduce_a_word() {
        let t = track("world hello");
        let mut plan = EditPlan::unedited(&t);
        plan.move_word(1, 0).unwrap();
        let spoken: Vec<String> = t.words.iter().map(|w| normalize(&w.text)).collect();
        assert!(plan.text(&t).split_whitespace().all(|w| spoken.contains(&normalize(w))));
    }

    #[test]
    fn moving_outside_the_kept_words_is_an_error() {
        let t = track("one two");
        let mut plan = EditPlan::unedited(&t);
        assert!(plan.move_word(0, 9).is_err());
    }

    #[test]
    fn removed_lists_what_the_edit_dropped() {
        let t = track("one two three four");
        let mut plan = EditPlan::unedited(&t);
        plan.remove(1);
        plan.remove(3);
        assert_eq!(plan.removed(&t), vec![1, 3]);
    }

    // -- audio spans --------------------------------------------------------

    #[test]
    fn untouched_speech_is_one_span_not_many() {
        let t = WordTrack::new(vec![
            Word::new(0, "one", 0, 100),
            Word::new(1, "two", 100, 200),
            Word::new(2, "three", 200, 300),
        ]);
        let plan = EditPlan { kept: vec![0, 1, 2], max_gap_ms: None, pad_ms: 0 };
        assert_eq!(
            plan.keep_intervals(&t),
            vec![Interval { start_ms: 0, end_ms: 300 }]
        );
    }

    #[test]
    fn a_removed_word_splits_the_audio_into_two_spans() {
        let t = WordTrack::new(vec![
            Word::new(0, "one", 0, 100),
            Word::new(1, "um", 100, 200),
            Word::new(2, "three", 200, 300),
        ]);
        let plan = EditPlan { kept: vec![0, 2], max_gap_ms: None, pad_ms: 0 };
        assert_eq!(
            plan.keep_intervals(&t),
            vec![
                Interval { start_ms: 0, end_ms: 100 },
                Interval { start_ms: 200, end_ms: 300 },
            ]
        );
    }

    #[test]
    fn padding_never_reaches_into_a_neighbouring_word() {
        // 40ms of silence between each word; a 30ms pad must clamp to 20ms so
        // it cannot pick up the tail of the removed word.
        let t = WordTrack::new(vec![
            Word::new(0, "one", 0, 100),
            Word::new(1, "um", 140, 240),
            Word::new(2, "three", 280, 380),
        ]);
        let plan = EditPlan { kept: vec![0, 2], max_gap_ms: None, pad_ms: 30 };
        let spans = plan.keep_intervals(&t);
        assert_eq!(spans[0].end_ms, 120, "bled into the silence before \"um\"");
        assert_eq!(spans[1].start_ms, 260, "bled into the silence after \"um\"");
        // And neither span touches the removed word's own audio.
        assert!(spans[0].end_ms <= 140);
        assert!(spans[1].start_ms >= 240);
    }

    #[test]
    fn padding_does_not_run_past_the_start_of_the_recording() {
        let t = WordTrack::new(vec![Word::new(0, "one", 10, 100)]);
        let plan = EditPlan { kept: vec![0], max_gap_ms: None, pad_ms: 30 };
        assert_eq!(plan.keep_intervals(&t)[0].start_ms, 0);
    }

    #[test]
    fn output_duration_is_the_sum_of_kept_spans() {
        let t = WordTrack::new(vec![
            Word::new(0, "one", 0, 100),
            Word::new(1, "um", 100, 200),
            Word::new(2, "three", 200, 300),
        ]);
        let plan = EditPlan { kept: vec![0, 2], max_gap_ms: None, pad_ms: 0 };
        assert_eq!(plan.output_duration_ms(&t), 200);
        assert_eq!(t.duration_ms(), 300);
    }

    #[test]
    fn a_long_pause_is_trimmed_to_the_gap_limit() {
        // Two kept, adjacent words with two seconds of dead air between them.
        let t = WordTrack::new(vec![
            Word::new(0, "one", 0, 100),
            Word::new(1, "two", 2100, 2200),
        ]);
        let plan = EditPlan { kept: vec![0, 1], max_gap_ms: Some(200), pad_ms: 0 };
        let spans = plan.keep_intervals(&t);
        assert_eq!(spans.len(), 2, "the pause should split the span");
        let silence_kept = (spans[0].end_ms - 100) + (2100 - spans[1].start_ms);
        assert_eq!(silence_kept, 200);
        assert_eq!(plan.output_duration_ms(&t), 400);
    }

    #[test]
    fn a_short_pause_is_left_alone() {
        let t = WordTrack::new(vec![
            Word::new(0, "one", 0, 100),
            Word::new(1, "two", 180, 280),
        ]);
        let plan = EditPlan { kept: vec![0, 1], max_gap_ms: Some(200), pad_ms: 0 };
        assert_eq!(
            plan.keep_intervals(&t),
            vec![Interval { start_ms: 0, end_ms: 280 }],
            "an 80ms pause is under the limit and needs no cut"
        );
    }

    #[test]
    fn reordered_words_are_separate_spans_in_playback_order() {
        let t = WordTrack::new(vec![
            Word::new(0, "world", 0, 100),
            Word::new(1, "hello", 100, 200),
        ]);
        let plan = EditPlan { kept: vec![1, 0], max_gap_ms: None, pad_ms: 0 };
        assert_eq!(
            plan.keep_intervals(&t),
            vec![
                Interval { start_ms: 100, end_ms: 200 },
                Interval { start_ms: 0, end_ms: 100 },
            ]
        );
    }

    // -- candidates and judgements ------------------------------------------

    #[test]
    fn an_unambiguous_transcript_raises_no_candidates() {
        let t = track("the quick brown fox jumps over the lazy dog");
        assert!(candidate_spans(&t, &[]).is_empty());
    }

    #[test]
    fn a_discourse_marker_is_proposed_not_removed() {
        let t = track("it basically tastes like chicken");
        let c = candidate_spans(&t, &[]);
        assert_eq!(
            c.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
            vec!["basically", "like"],
            "both are ambiguous; neither may be cut by rule alone"
        );
    }

    #[test]
    fn a_two_word_marker_is_one_candidate() {
        let t = track("you know we need more time");
        let c = candidate_spans(&t, &[]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].text, "you know");
        assert_eq!(c[0].indices, vec![0, 1]);
    }

    #[test]
    fn a_phrase_is_preferred_over_its_first_word_alone() {
        // "kind" is not in the ambiguous word list, but "kind of" is a phrase;
        // "sort of" would otherwise be proposed as a bare "sort".
        let t = track("it is sort of hard to say");
        let c = candidate_spans(&t, &[]);
        assert_eq!(c[0].text, "sort of");
        assert_eq!(c[0].indices, vec![2, 3]);
    }

    #[test]
    fn candidates_skip_words_already_removed() {
        let t = track("um so I went");
        let removals = propose_fillers(&t);
        let removed: Vec<u32> = removals.iter().map(|r| r.index).collect();
        let c = candidate_spans(&t, &removed);
        // "um" is gone by rule; only "so" is left to judge.
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].text, "so");
    }

    #[test]
    fn a_hesitation_between_the_halves_does_not_hide_a_phrase() {
        let t = track("you um know we need more time");
        let removed: Vec<u32> = propose_fillers(&t).iter().map(|r| r.index).collect();
        let c = candidate_spans(&t, &removed);
        assert_eq!(c[0].text, "you know", "matched over surviving words");
        assert_eq!(c[0].indices, vec![0, 2]);
    }

    #[test]
    fn the_judge_sees_the_sentence_as_it_will_be_heard() {
        let t = track("um so I went to the store");
        let removed: Vec<u32> = propose_fillers(&t).iter().map(|r| r.index).collect();
        let c = candidate_spans(&t, &removed);
        // "um" is hidden because it is already cut, and "so" is marked.
        assert_eq!(judge_context(&t, &removed, &c[0]), "<<so>> I went to the store");
    }

    #[test]
    fn the_judge_context_marks_a_phrase_as_one_unit() {
        let t = track("so the thing is you know we need time");
        let c = candidate_spans(&t, &[]);
        let phrase = c.iter().find(|c| c.text == "you know").unwrap();
        assert_eq!(
            judge_context(&t, &[], phrase),
            "so the thing is <<you know>> we need time"
        );
    }

    #[test]
    fn only_filler_verdicts_become_removals() {
        let t = track("it basically tastes like chicken");
        let c = candidate_spans(&t, &[]);
        let judged = vec![
            Judgement { candidate: c[0].clone(), verdict: Verdict::Filler },
            Judgement { candidate: c[1].clone(), verdict: Verdict::Content },
        ];
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &removals_from_judgements(&judged));
        assert_eq!(plan.text(&t), "it tastes like chicken");
    }

    #[test]
    fn a_filler_phrase_verdict_removes_every_word_of_it() {
        let t = track("you know we need more time");
        let c = candidate_spans(&t, &[]);
        let judged = vec![Judgement { candidate: c[0].clone(), verdict: Verdict::Content }];
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &removals_from_judgements(&judged));
        assert_eq!(plan.text(&t), "you know we need more time");

        let judged = vec![Judgement { candidate: c[0].clone(), verdict: Verdict::Filler }];
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &removals_from_judgements(&judged));
        assert_eq!(plan.text(&t), "we need more time");
    }

    #[test]
    fn judging_cannot_introduce_a_word() {
        // The judge's whole output space is two words, neither of which is text.
        let t = track("it basically tastes like chicken");
        let c = candidate_spans(&t, &[]);
        let judged: Vec<Judgement> = c
            .iter()
            .map(|c| Judgement { candidate: c.clone(), verdict: Verdict::Filler })
            .collect();
        let mut plan = EditPlan::unedited(&t);
        apply_removals(&mut plan, &removals_from_judgements(&judged));
        let spoken: Vec<String> = t.words.iter().map(|w| normalize(&w.text)).collect();
        assert!(plan.text(&t).split_whitespace().all(|w| spoken.contains(&normalize(w))));
        plan.validate(&t).unwrap();
    }

    #[test]
    fn a_verdict_round_trips_as_the_judge_sends_it() {
        assert_eq!(
            serde_json::from_str::<Verdict>(r#""filler""#).unwrap(),
            Verdict::Filler
        );
        assert_eq!(
            serde_json::from_str::<Verdict>(r#""content""#).unwrap(),
            Verdict::Content
        );
    }

    #[test]
    fn a_word_carries_no_confidence_unless_the_decoder_gave_one() {
        let w = Word::new(0, "hello", 0, 100);
        assert_eq!(w.confidence, None);
        assert_eq!(w.with_confidence(0.92).confidence, Some(0.92));
    }

    #[test]
    fn confidence_is_omitted_from_the_wire_when_absent() {
        let w = Word::new(3, "hello", 0, 100);
        let json = serde_json::to_string(&w).unwrap();
        assert!(!json.contains("confidence"), "{json}");
        assert_eq!(serde_json::from_str::<Word>(&json).unwrap(), w);
    }

    #[test]
    fn a_word_deserialises_without_a_confidence_field() {
        let w: Word =
            serde_json::from_str(r#"{"index":0,"text":"hi","start_ms":0,"end_ms":100}"#).unwrap();
        assert_eq!(w.confidence, None);
    }

    // -- serde --------------------------------------------------------------

    #[test]
    fn a_plan_survives_a_round_trip() {
        let plan = EditPlan { kept: vec![0, 2, 5], max_gap_ms: Some(250), pad_ms: 30 };
        let json = serde_json::to_string(&plan).unwrap();
        assert_eq!(serde_json::from_str::<EditPlan>(&json).unwrap(), plan);
    }

    #[test]
    fn a_proposal_parses_from_the_model_s_json() {
        let json = r#"{"deletions":[{"index":0,"word":"um","reason":"hesitation"}]}"#;
        let p: DeletionProposal = serde_json::from_str(json).unwrap();
        assert_eq!(p.deletions[0].reason, Some(RemovalReason::Hesitation));
    }

    #[test]
    fn a_proposal_parses_without_a_reason() {
        let json = r#"{"deletions":[{"index":0,"word":"um"}]}"#;
        let p: DeletionProposal = serde_json::from_str(json).unwrap();
        assert_eq!(p.deletions[0].reason, None);
    }
}

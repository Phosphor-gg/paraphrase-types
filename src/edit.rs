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
//!
//! There are deliberately no rules about *which* words are worth cutting. An
//! earlier version of this module carried lists of hesitation sounds and
//! discourse markers and asked a model only to adjudicate the spans those lists
//! found. That bounded the AI's recall by the lists: a rambling clause, a
//! redundant restatement or a worthwhile reordering is on no list, so the model
//! was never even asked. The model now decides everything, and nothing it says
//! is trusted: there are two ways to ask it, and each one is safe by
//! construction rather than by validation.
//!
//! [`apply_cuts`] takes a list of quotations to delete and returns the indices
//! the quotes did not claim, so the AI's output space is subtraction from a set.
//! [`bind_edit`] takes the passage rewritten as free text and makes every word
//! of it claim one not-yet-claimed original word, which also allows reordering
//! and costs the whole window when one word will not bind.

use serde::{Deserialize, Serialize};

/// Default padding kept either side of a word so a cut does not clip its edges.
pub const DEFAULT_PAD_MS: u32 = 30;

/// A transcribed word and the span of audio that produced it.
///
/// Unknown fields are rejected for the same reason [`crate::asr::TranscribeResponse`]
/// rejects them: this is the shape the GPU service produces, and a field renamed
/// on the Python side would otherwise arrive as a silent `None`. For
/// `confidence` that would quietly disable the editor's highlighting with
/// nothing to notice, which is worse than a failed request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// Who dropped a word.
///
/// There is nothing finer to record. The model is not asked to categorise its
/// own reasoning, because a category it invents is unverifiable and the editor
/// only needs to distinguish "the AI did this" from "you did this" so a person
/// knows what to review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalReason {
    Ai,
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


/// A word the edit dropped, and who dropped it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removal {
    pub index: u32,
    pub reason: RemovalReason,
}

/// Apply removals to a plan.
pub fn apply_removals(plan: &mut EditPlan, removals: &[Removal]) {
    let drop: std::collections::HashSet<u32> = removals.iter().map(|r| r.index).collect();
    plan.kept.retain(|k| !drop.contains(k));
}

// ---------------------------------------------------------------------------
// Binding an edited transcript back onto the recording
// ---------------------------------------------------------------------------

/// Why an edited transcript could not be bound to the recording.
///
/// Both variants mean the same thing in practice: the model rewrote instead of
/// editing. They are separate because the distinction tells you *how* it went
/// wrong, which is worth knowing when a prompt needs changing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BindError {
    /// A word that does not occur in this passage at all.
    NotSpoken { word: String },
    /// A word used more times than it was spoken.
    ///
    /// Its own variant because it is the signature of a model duplicating a
    /// phrase rather than inventing vocabulary, and there is only one piece of
    /// audio per occurrence to cut.
    UsedMoreOftenThanSpoken { word: String },
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSpoken { word } => {
                write!(f, "{word:?} was never spoken in this passage")
            }
            Self::UsedMoreOftenThanSpoken { word } => {
                write!(f, "{word:?} is used more often than it was spoken")
            }
        }
    }
}

impl std::error::Error for BindError {}

/// Bind an edited passage back onto the words that were actually spoken.
///
/// This is the whole safety mechanism, and it is why the model needs no rules
/// and no restrictions on what it may cut or move. Every word of the edited text
/// must claim one not-yet-claimed original word; the result is the sequence of
/// original indices those claims landed on. Deleting is a word left unclaimed
/// and reordering is claims made out of order, so both are free — while a word
/// nobody said has nothing to claim and is rejected.
///
/// Rejecting is the point. The earlier Python prototype of this feature aligned
/// the model's text with a diff and cut everything the diff called changed, so a
/// rephrase silently became a cut of real speech and nobody could tell. An
/// unbindable word is an error here, never a deletion.
///
/// Returned indices are absolute, taken from each [`Word::index`], so a window
/// into the middle of a recording needs no offset arithmetic at the call site.
pub fn bind_edit(window: &[Word], edited: &str) -> Result<Vec<u32>, BindError> {
    use std::collections::{HashMap, VecDeque};

    let mut available: HashMap<String, VecDeque<usize>> = HashMap::new();
    for (position, word) in window.iter().enumerate() {
        let normalised = normalize(&word.text);
        if normalised.is_empty() {
            continue;
        }
        available.entry(normalised).or_default().push_back(position);
    }

    let mut kept = Vec::new();
    let mut cursor = 0usize;

    for token in edited.split_whitespace() {
        let want = normalize(token);
        // Punctuation-only tokens carry no audio, so they bind to nothing and
        // are not an error either.
        if want.is_empty() {
            continue;
        }
        let Some(slots) = available.get_mut(&want) else {
            return Err(BindError::NotSpoken { word: token.to_string() });
        };
        if slots.is_empty() {
            return Err(BindError::UsedMoreOftenThanSpoken { word: token.to_string() });
        }
        // The nearest unclaimed occurrence at or after the cursor, falling back
        // to the earliest remaining one. Preferring forwards means an unchanged
        // passage binds to itself, so the common case stays in recorded order
        // and only a genuine move produces a backwards jump.
        let choice = slots.iter().position(|&p| p >= cursor).unwrap_or(0);
        let position = slots.remove(choice).expect("position() returned a valid index");
        kept.push(window[position].index);
        cursor = position + 1;
    }

    Ok(kept)
}

/// Whether a kept sequence plays words out of their recorded order.
///
/// Worth knowing before rendering: splicing non-adjacent spans out of
/// chronological order is audibly worse than deleting between them, because
/// pitch and pace do not match across the join and no crossfade hides it.
pub fn is_reordered(kept: &[u32]) -> bool {
    kept.windows(2).any(|w| w[0] >= w[1])
}

// ---------------------------------------------------------------------------
// Cutting by quotation
// ---------------------------------------------------------------------------

/// The model's reply on the cut path: the pieces of the passage it wants gone,
/// quoted from the passage itself.
///
/// Deletion is the only operation the format has, which is why this is one
/// field of strings and not a patch language. `{"cuts": []}` is the ordinary
/// answer for a passage already clean.
///
/// Unknown fields are *not* denied here, unlike [`Word`]: that shape comes from
/// the GPU service, where a renamed field has to fail loudly, while this one
/// comes from a language model, where an extra chatty key is noise and refusing
/// the whole reply over it would throw away a usable edit.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CutList {
    pub cuts: Vec<String>,
}

/// A run of the track one accepted cut removes, in absolute [`Word::index`]
/// values, half-open.
///
/// Absolute to match [`bind_edit`]'s convention, half-open to match
/// [`chunk_ranges`], so a window into the middle of a recording needs no offset
/// arithmetic at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CutSpan {
    pub start: u32,
    pub end: u32,
}

/// Why one quote was not cut.
///
/// A refusal leaves those words in the recording, so every variant here is the
/// safe outcome of the pair: keeping a word someone wanted gone costs them one
/// click, and cutting a word they wanted removes audio they may not notice is
/// missing until they listen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CutError {
    /// The quote normalises to nothing: `"..."`, or whitespace.
    NoWords,
    /// No contiguous run of the passage reads like this. The usual cause is a
    /// model paraphrasing the words it meant to quote.
    NotFound,
    /// Several runs match and they are not all adjacent, so which one was meant
    /// changes what the passage says. The quote carries nothing to choose with.
    Ambiguous { occurrences: usize },
    /// Every matching run is already covered by an accepted cut. Also how a
    /// duplicated entry lands.
    AlreadyCut,
    /// The quote names every word of the passage, which is a summary refusing
    /// to be one rather than an edit.
    WholeWindow,
}

impl std::fmt::Display for CutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWords => write!(f, "has no words in it"),
            Self::NotFound => write!(f, "does not occur in this passage"),
            Self::Ambiguous { occurrences } => write!(
                f,
                "occurs {occurrences} times in different places, and which was meant \
                 changes what the passage says"
            ),
            Self::AlreadyCut => write!(f, "is already covered by a cut"),
            Self::WholeWindow => write!(f, "is the whole passage"),
        }
    }
}

impl std::error::Error for CutError {}

/// A quote that was dropped, kept verbatim with its reason.
///
/// The text is stored as the model wrote it so a log line or a prompt change
/// has the actual string to look at, and `ordinal` is its position in the reply
/// so a rejection can be shown next to the entry that caused it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedCut {
    pub ordinal: usize,
    pub quote: String,
    pub error: CutError,
}

impl std::fmt::Display for RejectedCut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} {}", self.quote, self.error)
    }
}

/// What a cut list did to one window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutOutcome {
    /// Original indices that survive, in recorded order.
    pub kept: Vec<u32>,
    pub applied: Vec<CutSpan>,
    pub rejected: Vec<RejectedCut>,
}

/// Find the one run of the window a quote names.
///
/// Matching is over the window's *content* words, each passed through
/// [`normalize`], so a quote survives re-punctuation and case and the two AI
/// paths cannot drift on what counts as the same word. `claimed` marks window
/// positions already taken by an accepted cut; a shorter slice than the window,
/// `&[]` included, means those positions are free.
///
/// The returned range is window positions, half-open, and spans from the first
/// matched word to the last, so a punctuation-only word sitting inside the
/// phrase is swallowed with it.
///
/// ADJACENCY IS AN IDENTITY, NOT A PREFERENCE. When a quote occupies two runs
/// back to back, deleting either leaves the same sequence of words, so there is
/// nothing to get wrong and the earliest is taken. Occurrences separated by
/// other words give different sequences, and the quote says nothing about which
/// was meant, so they are refused. Without this rule the commonest false start
/// in speech, "I went to the I went to the station", is ambiguous and survives
/// untouched.
///
/// Taking the earliest of an adjacent run is a hypothesis about audio rather
/// than a measurement: in a stutter the aborted attempt comes first and the
/// clean one second, so the later should sound better. The kept text is
/// identical either way.
///
/// There is deliberately no minimum quote length, no stopword list and no fuzzy
/// matching. Length is only a proxy for uniqueness and uniqueness is checked
/// directly, so a one-word quote is as safe as a ten-word one, and a floor
/// would refuse the one-word quotes a model actually produces for "uh" and
/// "basically".
pub fn locate_cut(
    window: &[Word],
    quote: &str,
    claimed: &[bool],
) -> Result<std::ops::Range<usize>, CutError> {
    let content: Vec<(usize, String)> = window
        .iter()
        .enumerate()
        .filter_map(|(position, word)| {
            let normalised = normalize(&word.text);
            (!normalised.is_empty()).then_some((position, normalised))
        })
        .collect();

    let needle: Vec<String> = quote
        .split_whitespace()
        .map(normalize)
        .filter(|n| !n.is_empty())
        .collect();
    if needle.is_empty() {
        return Err(CutError::NoWords);
    }
    if needle.len() > content.len() {
        return Err(CutError::NotFound);
    }

    // The scan does not skip past a hit, so a self-overlapping quote like "the
    // the" in "the the the" is seen as the several overlapping choices it
    // really is and refused below, rather than silently resolved.
    let hits: Vec<std::ops::Range<usize>> = (0..=content.len() - needle.len())
        .filter(|&at| {
            content[at..at + needle.len()].iter().zip(&needle).all(|((_, w), n)| w == n)
        })
        .map(|at| content[at].0..content[at + needle.len() - 1].0 + 1)
        .collect();
    if hits.is_empty() {
        return Err(CutError::NotFound);
    }

    let free: Vec<std::ops::Range<usize>> = hits
        .into_iter()
        .filter(|hit| !hit.clone().any(|p| claimed.get(p).copied().unwrap_or(false)))
        .collect();
    let Some(chosen) = free.first().cloned() else {
        return Err(CutError::AlreadyCut);
    };
    if free.len() > 1 && !free.windows(2).all(|pair| pair[0].end == pair[1].start) {
        return Err(CutError::Ambiguous { occurrences: free.len() });
    }
    if chosen.clone().filter(|&p| !normalize(&window[p].text).is_empty()).count()
        == content.len()
    {
        return Err(CutError::WholeWindow);
    }
    Ok(chosen)
}

/// Apply a cut list to a window, dropping the entries that cannot be placed
/// exactly once.
///
/// This is the whole safety mechanism of the cut path, and it is structural
/// rather than a validation pass. `kept` holds `u32` taken only from
/// [`Word::index`]; the reply holds `String` and nothing parses one into the
/// other. A quote is a query that selects positions, the output is the
/// complement of the positions it selected, and subtracting from a set cannot
/// add a member. So for any cut list whatsoever, including an adversarial one,
/// the result is a subsequence of the window's own indices in recorded order:
/// a word nobody said is not forbidden here, it is unrepresentable. Reordering
/// is unrepresentable for the same reason, which is a real narrowing of what
/// the AI may propose, not a formality.
///
/// Total and infallible on purpose. No input can fail the whole call, so
/// partial application is in the signature rather than in a convention: a
/// refused quote claims nothing, its words survive, and every other entry still
/// applies. Nothing in the refusal path can ever cut *more* than was asked for,
/// because two overlapping quotes drop the second rather than cutting the union.
///
/// Entries are located against the pristine window, never against text
/// progressively rewritten by earlier cuts; only the claimed-position set
/// accumulates. Order therefore matters solely for which entry wins a contested
/// span, and reply order is the model's own ranking.
pub fn apply_cuts(window: &[Word], cuts: &[String]) -> CutOutcome {
    let mut claimed = vec![false; window.len()];
    let mut applied = Vec::new();
    let mut rejected = Vec::new();

    for (ordinal, quote) in cuts.iter().enumerate() {
        match locate_cut(window, quote, &claimed) {
            Ok(span) => {
                applied.push(CutSpan {
                    start: window[span.start].index,
                    end: window[span.end - 1].index + 1,
                });
                for position in span {
                    claimed[position] = true;
                }
            }
            Err(error) => {
                rejected.push(RejectedCut { ordinal, quote: quote.clone(), error })
            }
        }
    }

    let kept = window
        .iter()
        .enumerate()
        .filter(|(position, _)| !claimed[*position])
        .map(|(_, word)| word.index)
        .collect();

    CutOutcome { kept, applied, rejected }
}

/// Whether any word with audio survives a cut list.
///
/// A window-level check, because the per-quote [`CutError::WholeWindow`] guard
/// only sees one entry at a time and two entries can cover a passage between
/// them.
pub fn keeps_any_word(window: &[Word], kept: &[u32]) -> bool {
    let surviving: std::collections::HashSet<u32> = kept.iter().copied().collect();
    window
        .iter()
        .any(|w| surviving.contains(&w.index) && !normalize(&w.text).is_empty())
}

/// Split a track into windows to edit separately.
///
/// A whole recording in one prompt degrades the model's attention to any
/// particular sentence, and a megabyte of transcript is slow besides. Windows
/// are also independent, so they can be edited concurrently.
///
/// Boundaries prefer the end of a sentence in the last quarter of the window,
/// because a cut landing mid-clause removes the context needed to judge the
/// words either side of it. Reordering cannot cross a window boundary, which is
/// an accepted limit: moving a clause between distant parts of a recording is
/// not something this feature promises.
pub fn chunk_ranges(track: &WordTrack, target_words: usize) -> Vec<(usize, usize)> {
    if track.is_empty() {
        return Vec::new();
    }
    let target = target_words.max(1);
    let mut out = Vec::new();
    let mut start = 0usize;

    while start < track.len() {
        let limit = (start + target).min(track.len());
        if limit == track.len() {
            out.push((start, limit));
            break;
        }
        let earliest = start + (target * 3 / 4).max(1);
        let mut end = limit;
        for position in (earliest..limit).rev() {
            if ends_sentence(&track.words[position].text) {
                end = position + 1;
                break;
            }
        }
        out.push((start, end));
        start = end;
    }
    out
}

fn ends_sentence(text: &str) -> bool {
    matches!(text.trim_end().chars().last(), Some('.') | Some('!') | Some('?'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(text: &str) -> WordTrack {
        WordTrack::from_text(text, 100)
    }

    fn spoken(track: &WordTrack) -> Vec<String> {
        track.words.iter().map(|w| normalize(&w.text)).collect()
    }

    // -- the load-bearing invariant -----------------------------------------

    #[test]
    fn a_bound_edit_can_only_contain_words_that_were_spoken() {
        let t = track("um so I basically went to the store yesterday");
        let kept = bind_edit(&t.words, "I went to the store yesterday").unwrap();
        let plan = EditPlan { kept, max_gap_ms: None, pad_ms: 30 };
        plan.validate(&t).unwrap();
        for word in plan.text(&t).split_whitespace() {
            assert!(spoken(&t).contains(&normalize(word)), "{word:?} was not spoken");
        }
    }

    #[test]
    fn an_invented_word_is_rejected_rather_than_cut() {
        // The failure that made the earlier prototype unusable: the model
        // rewrites, and the rewrite is charged to the audio as a deletion.
        let t = track("um I went to the store");
        assert_eq!(
            bind_edit(&t.words, "I subsequently went to the store"),
            Err(BindError::NotSpoken { word: "subsequently".into() })
        );
    }

    #[test]
    fn a_wholesale_rephrase_is_rejected() {
        let t = track("um so I kind of went to the store yesterday");
        assert!(bind_edit(&t.words, "I visited the shop").is_err());
    }

    #[test]
    fn a_word_cannot_be_used_more_often_than_it_was_spoken() {
        // There is one piece of audio per occurrence.
        let t = track("I went to the store");
        assert_eq!(
            bind_edit(&t.words, "I went to the the store"),
            Err(BindError::UsedMoreOftenThanSpoken { word: "the".into() })
        );
    }

    #[test]
    fn a_duplicated_phrase_is_rejected() {
        let t = track("we should ship it");
        assert!(matches!(
            bind_edit(&t.words, "we should ship it we should ship it"),
            Err(BindError::UsedMoreOftenThanSpoken { .. })
        ));
    }

    #[test]
    fn every_bind_error_names_the_offending_word() {
        for e in [
            BindError::NotSpoken { word: "furthermore".into() },
            BindError::UsedMoreOftenThanSpoken { word: "the".into() },
        ] {
            let m = e.to_string();
            assert!(m.contains('"'), "{m:?} should quote the word");
        }
    }

    // -- what the model is free to do ---------------------------------------

    #[test]
    fn an_unchanged_passage_binds_to_itself() {
        let t = track("the quick brown fox jumps over the lazy dog");
        let kept = bind_edit(&t.words, "the quick brown fox jumps over the lazy dog").unwrap();
        assert_eq!(kept, (0..9).collect::<Vec<u32>>());
        assert!(!is_reordered(&kept));
    }

    #[test]
    fn deleting_anything_is_free_with_no_rule_saying_what() {
        // A rambling clause is on no filler list, and that is the point: the
        // model may cut it and the binding does not care why.
        let t = track("the point is and I should say this first that it works");
        let kept = bind_edit(&t.words, "the point is that it works").unwrap();
        let plan = EditPlan { kept, max_gap_ms: None, pad_ms: 0 };
        assert_eq!(plan.text(&t), "the point is that it works");
    }

    #[test]
    fn reordering_is_allowed() {
        let t = track("world hello");
        let kept = bind_edit(&t.words, "hello world").unwrap();
        assert_eq!(kept, vec![1, 0]);
        assert!(is_reordered(&kept));
    }

    #[test]
    fn a_clause_can_be_moved() {
        let t = track("because it was raining we stayed in");
        let kept = bind_edit(&t.words, "we stayed in because it was raining").unwrap();
        assert_eq!(kept, vec![4, 5, 6, 0, 1, 2, 3]);
        assert!(is_reordered(&kept));
        let plan = EditPlan { kept, max_gap_ms: None, pad_ms: 0 };
        plan.validate(&t).unwrap();
    }

    #[test]
    fn repunctuation_and_case_do_not_break_the_binding() {
        let t = track("um i went to the store");
        let kept = bind_edit(&t.words, "I went to the store.").unwrap();
        assert_eq!(kept, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_punctuation_only_token_binds_to_nothing_and_is_not_an_error() {
        let t = track("hello there");
        assert_eq!(bind_edit(&t.words, "hello , there").unwrap(), vec![0, 1]);
    }

    #[test]
    fn a_repeated_word_binds_to_the_nearest_unclaimed_occurrence() {
        // "the" occurs twice; an unchanged passage must stay in order rather
        // than binding the second mention to the first occurrence.
        let t = track("the cat sat on the mat");
        let kept = bind_edit(&t.words, "the cat sat on the mat").unwrap();
        assert_eq!(kept, vec![0, 1, 2, 3, 4, 5]);
        assert!(!is_reordered(&kept));
    }

    #[test]
    fn a_kept_stutter_binds_forwards_not_backwards() {
        let t = track("I I went");
        let kept = bind_edit(&t.words, "I went").unwrap();
        // Either "I" is defensible, but it must not then report a reorder.
        assert_eq!(kept.len(), 2);
        assert!(!is_reordered(&kept), "{kept:?}");
    }

    #[test]
    fn an_empty_edit_binds_to_nothing() {
        // Legitimate for a window that held only filler; a whole recording
        // reduced to nothing is caught by the caller, not here.
        let t = track("um uh er");
        assert_eq!(bind_edit(&t.words, "").unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn a_window_returns_absolute_indices() {
        // So a window into the middle of a recording needs no offset
        // arithmetic at the call site, which is where an off-by-one would cut
        // the wrong audio.
        let t = track("zero one two three four five");
        let kept = bind_edit(&t.words[3..], "three five").unwrap();
        assert_eq!(kept, vec![3, 5]);
    }

    // -- cutting by quotation -----------------------------------------------

    fn quotes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// The cut path's kept indices as text, which is what a person hears.
    fn cut_text(t: &WordTrack, cuts: &[&str]) -> String {
        let outcome = apply_cuts(&t.words, &quotes(cuts));
        EditPlan { kept: outcome.kept, max_gap_ms: None, pad_ms: 0 }.text(t)
    }

    fn reasons(t: &WordTrack, cuts: &[&str]) -> Vec<CutError> {
        apply_cuts(&t.words, &quotes(cuts)).rejected.into_iter().map(|r| r.error).collect()
    }

    #[test]
    fn a_cut_list_can_only_remove_words_that_were_spoken() {
        let t = track("um so I basically went to the store yesterday");
        let outcome = apply_cuts(&t.words, &quotes(&["um so", "basically"]));
        let plan = EditPlan { kept: outcome.kept, max_gap_ms: None, pad_ms: 30 };
        plan.validate(&t).unwrap();
        assert_eq!(plan.text(&t), "I went to the store yesterday");
        for word in plan.text(&t).split_whitespace() {
            assert!(spoken(&t).contains(&normalize(word)), "{word:?} was not spoken");
        }
    }

    #[test]
    fn an_invented_quote_cuts_nothing_at_all() {
        // The failure that made the earlier prototype unusable, in the shape
        // this format can take it: a model that paraphrases what it meant to
        // quote must not cut whatever sits nearest.
        let t = track("um so I went to the store");
        let outcome = apply_cuts(&t.words, &quotes(&["strolled to the store"]));
        assert_eq!(outcome.kept, (0..7).collect::<Vec<u32>>());
        assert!(outcome.applied.is_empty());
        assert_eq!(outcome.rejected[0].error, CutError::NotFound);
    }

    #[test]
    fn an_ambiguous_quote_is_refused_and_its_words_survive() {
        // "the" twice, separated: deleting one says something different from
        // deleting the other, and the quote does not say which was meant.
        let t = track("the cat sat on the mat");
        assert_eq!(cut_text(&t, &["the"]), "the cat sat on the mat");
        assert_eq!(reasons(&t, &["the"]), vec![CutError::Ambiguous { occurrences: 2 }]);
    }

    #[test]
    fn overlapping_occurrences_are_refused_even_though_either_would_do() {
        // Conservative on purpose: both choices here leave "the the cat", so
        // the refusal costs recall, and widening the rule to notice that is how
        // a locator stops being provable.
        let t = track("the the the cat");
        assert_eq!(reasons(&t, &["the the"]), vec![CutError::Ambiguous { occurrences: 2 }]);
        assert_eq!(cut_text(&t, &["the the"]), "the the the cat");
    }

    #[test]
    fn an_adjacent_repetition_is_cut_once_because_either_copy_leaves_the_same_words() {
        // The commonest false start in speech. The quote matches twice, back to
        // back, and deleting either leaves the identical sequence, so there is
        // nothing to get wrong. Refusing it here would leave the passage
        // verbatim and lose the case outright.
        let t = track("I went to the I went to the station");
        assert_eq!(cut_text(&t, &["I went to the"]), "I went to the station");
        let outcome = apply_cuts(&t.words, &quotes(&["I went to the"]));
        assert_eq!(outcome.applied, vec![CutSpan { start: 0, end: 4 }]);
        assert!(outcome.rejected.is_empty());
    }

    #[test]
    fn a_periodic_repetition_is_cut_once() {
        let t = track("we should ship it we should ship it we should ship it");
        assert_eq!(cut_text(&t, &["we should ship it"]), "we should ship it we should ship it");
    }

    #[test]
    fn a_one_word_quote_is_fine_when_it_occurs_once() {
        // No length floor: length is only a proxy for uniqueness, and
        // uniqueness is checked directly.
        let t = track("it basically tastes like chicken");
        assert_eq!(cut_text(&t, &["basically"]), "it tastes like chicken");
    }

    #[test]
    fn quoting_both_copies_of_a_repetition_cuts_both() {
        // Stated rather than fixed, and the one place this format is worse than
        // free text: a quote cannot distinguish "delete this span" from
        // "collapse this repetition", so a model that quotes both copies loses
        // the words the speaker meant to say. The prompt pushes one-copy
        // quotes; a locator rule that halved a doubled quote would make the
        // format's single semantic untrue, and "very very" is a real case where
        // both copies should go.
        let t = track("I I went to the shop");
        assert_eq!(cut_text(&t, &["I"]), "I went to the shop");
        assert_eq!(cut_text(&t, &["I I"]), "went to the shop");
    }

    #[test]
    fn a_punctuation_only_quote_has_nothing_to_cut() {
        let t = track("um so I went to the store");
        assert_eq!(reasons(&t, &["..."]), vec![CutError::NoWords]);
        assert_eq!(reasons(&t, &["   "]), vec![CutError::NoWords]);
    }

    #[test]
    fn punctuation_inside_a_quote_is_swallowed_and_outside_it_survives() {
        // A word carrying no letters carries no audio worth keeping on its own,
        // so it goes with the phrase it sits inside; a dash the quote stops
        // short of is left where it is.
        let t = track("so what I wanted to say is -- the deadline is Friday");
        assert_eq!(cut_text(&t, &["what I wanted to say is -- the"]), "so deadline is Friday");
        assert_eq!(cut_text(&t, &["so what I wanted to say is"]), "-- the deadline is Friday");
    }

    #[test]
    fn a_quote_naming_the_whole_passage_is_refused() {
        // Measured on qwen3:8b: a model asked to clean a short sentence
        // sometimes quotes all of it, which is a summary refusing to be one.
        let t = track("hello world");
        assert_eq!(reasons(&t, &["hello world"]), vec![CutError::WholeWindow]);
        let punctuated = track("hello . world");
        assert_eq!(reasons(&punctuated, &["hello . world"]), vec![CutError::WholeWindow]);
    }

    #[test]
    fn a_duplicate_entry_is_dropped_rather_than_cutting_twice() {
        let t = track("um so I went to the store");
        let outcome = apply_cuts(&t.words, &quotes(&["um so", "um so"]));
        assert_eq!(outcome.applied, vec![CutSpan { start: 0, end: 2 }]);
        assert_eq!(outcome.rejected[0].error, CutError::AlreadyCut);
        assert_eq!(
            EditPlan { kept: outcome.kept, max_gap_ms: None, pad_ms: 0 }.text(&t),
            "I went to the store"
        );
    }

    #[test]
    fn one_bad_entry_does_not_cost_the_good_ones() {
        // The whole motivation for leaving free text: there, one unbindable
        // word lost the entire window to verbatim.
        let t = track("um so I basically went to the uh store yesterday");
        let outcome =
            apply_cuts(&t.words, &quotes(&["um so", "hurried", "basically", "uh"]));
        assert_eq!(
            EditPlan { kept: outcome.kept, max_gap_ms: None, pad_ms: 0 }.text(&t),
            "I went to the store yesterday"
        );
        assert_eq!(outcome.applied.len(), 3);
        assert_eq!(outcome.rejected.len(), 1);
        assert_eq!(outcome.rejected[0].ordinal, 1, "the rejection names the entry");
        assert_eq!(outcome.rejected[0].quote, "hurried");
    }

    #[test]
    fn two_overlapping_quotes_cut_the_first_not_the_union() {
        // Nothing in the refusal path may ever cut more than was asked for.
        let t = track("the plan is we freeze writes on Friday");
        let outcome = apply_cuts(&t.words, &quotes(&["plan is we", "is we freeze"]));
        assert_eq!(
            EditPlan { kept: outcome.kept, max_gap_ms: None, pad_ms: 0 }.text(&t),
            "the freeze writes on Friday"
        );
        assert_eq!(outcome.rejected[0].error, CutError::AlreadyCut);
    }

    #[test]
    fn an_empty_cut_list_changes_nothing() {
        // The ordinary answer for a clean passage, and distinguishable from an
        // unusable reply, which is why this path needs no sentinel phrase.
        let t = track("the quick brown fox jumps over the lazy dog");
        let outcome = apply_cuts(&t.words, &[]);
        assert_eq!(outcome.kept, (0..9).collect::<Vec<u32>>());
        assert!(outcome.applied.is_empty() && outcome.rejected.is_empty());
        assert!(EditPlan { kept: outcome.kept, max_gap_ms: None, pad_ms: 0 }.is_unedited(&t));
    }

    #[test]
    fn re_punctuation_and_case_do_not_break_a_quote() {
        let t = WordTrack::new(vec![
            Word::new(0, "Um,", 0, 100),
            Word::new(1, "so", 100, 200),
            Word::new(2, "I", 200, 300),
            Word::new(3, "went", 300, 400),
        ]);
        assert_eq!(cut_text(&t, &["um so"]), "I went");
    }

    #[test]
    fn a_cut_list_cannot_reorder_anything() {
        // A capability the cut format genuinely gives up: `kept` is the window
        // minus a set, so it is always in recorded order whatever the model
        // asks for. `move_word` and the hand editor still reorder.
        let t = track("because it was raining we stayed in");
        let outcome = apply_cuts(&t.words, &quotes(&["we stayed in because it was raining"]));
        assert!(!is_reordered(&outcome.kept));
        assert_eq!(outcome.rejected[0].error, CutError::NotFound, "not a reorder, a non-match");
    }

    #[test]
    fn a_window_yields_absolute_indices() {
        let t = track("zero one two three four five");
        let outcome = apply_cuts(&t.words[3..], &quotes(&["four"]));
        assert_eq!(outcome.kept, vec![3, 5]);
        assert_eq!(outcome.applied, vec![CutSpan { start: 4, end: 5 }]);
    }

    #[test]
    fn a_rejection_prints_the_quote_that_caused_it() {
        let t = track("the cat sat on the mat");
        let rejected = apply_cuts(&t.words, &quotes(&["the"])).rejected;
        let message = rejected[0].to_string();
        assert!(message.contains("\"the\""), "{message:?} should quote the text");
        assert!(message.contains("occurs 2 times"), "{message:?} should say why");
    }

    #[test]
    fn a_window_cut_to_nothing_is_detectable_even_when_no_entry_was_refused() {
        // The per-quote whole-passage guard sees one entry at a time, so two
        // entries can still cover a passage between them. Measured on qwen3:8b.
        let t = track("um so I went to the store");
        let outcome = apply_cuts(&t.words, &quotes(&["um so I went", "to the store"]));
        assert!(outcome.rejected.is_empty());
        assert!(!keeps_any_word(&t.words, &outcome.kept));
        assert!(keeps_any_word(&t.words, &apply_cuts(&t.words, &quotes(&["um so"])).kept));
    }

    #[test]
    fn punctuation_left_alone_does_not_count_as_a_surviving_word() {
        // Otherwise a gutted window looks survivable because a comma is left.
        let t = track("um -- uh");
        let outcome = apply_cuts(&t.words, &quotes(&["um", "uh"]));
        assert_eq!(outcome.kept, vec![1]);
        assert!(!keeps_any_word(&t.words, &outcome.kept));
    }

    #[test]
    fn any_cut_list_at_all_yields_a_subsequence_of_the_window() {
        // The invariant as a property rather than a case. Adversarial quotes:
        // real slices, reversed slices, invented words, overlaps, repeats,
        // punctuation, the whole passage.
        let t = track("um so I think the budget is 40000 pounds which is a lot of money");
        let passage = t.text();
        let words: Vec<&str> = passage.split_whitespace().collect();
        let mut seed = 0x5DEECE66u64;
        let mut next = move |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n.max(1)
        };

        for _ in 0..2000 {
            let cuts: Vec<String> = (0..next(5))
                .map(|_| {
                    let at = next(words.len());
                    let len = 1 + next(5);
                    let mut piece: Vec<String> = words[at..(at + len).min(words.len())]
                        .iter()
                        .map(|w| w.to_string())
                        .collect();
                    match next(6) {
                        0 => piece.push("subsequently".into()),
                        1 => piece.reverse(),
                        2 => piece.push("...".into()),
                        3 => piece = words.iter().map(|w| w.to_string()).collect(),
                        _ => {}
                    }
                    piece.join(" ")
                })
                .collect();

            let outcome = apply_cuts(&t.words, &cuts);
            let plan = EditPlan { kept: outcome.kept.clone(), max_gap_ms: None, pad_ms: 0 };
            plan.validate(&t).expect("a cut list cannot produce an invalid plan");
            assert!(!is_reordered(&outcome.kept), "cuts cannot reorder: {cuts:?}");
            assert!(
                outcome.kept.windows(2).all(|w| w[0] < w[1]),
                "kept must be strictly increasing: {cuts:?}"
            );
            for &i in &outcome.kept {
                assert!(t.get(i).is_some(), "kept an index outside the window: {cuts:?}");
            }
            for span in &outcome.applied {
                assert!(span.start < span.end && (span.end as usize) <= t.len());
            }
            assert_eq!(
                outcome.applied.len() + outcome.rejected.len(),
                cuts.len(),
                "every entry is either applied or reported: {cuts:?}"
            );
        }
    }

    #[test]
    fn a_reply_parses_even_with_an_extra_field() {
        // The producer is a language model, so a chatty key is noise; refusing
        // the reply over it would throw away a usable edit.
        let list: CutList =
            serde_json::from_str(r#"{"cuts":["um so"],"note":"I cut the filler"}"#).unwrap();
        assert_eq!(list.cuts, vec!["um so".to_string()]);
        assert_eq!(serde_json::from_str::<CutList>(r#"{"cuts":[]}"#).unwrap().cuts.len(), 0);
    }

    #[test]
    fn a_rejected_cut_survives_a_round_trip_to_the_editor() {
        let rejected = RejectedCut {
            ordinal: 2,
            quote: "the".into(),
            error: CutError::Ambiguous { occurrences: 3 },
        };
        let json = serde_json::to_string(&rejected).unwrap();
        assert!(json.contains(r#""kind":"ambiguous""#), "{json}");
        assert_eq!(serde_json::from_str::<RejectedCut>(&json).unwrap(), rejected);
    }

    // -- windows ------------------------------------------------------------

    #[test]
    fn a_short_track_is_one_window() {
        let t = track("one two three");
        assert_eq!(chunk_ranges(&t, 100), vec![(0, 3)]);
    }

    #[test]
    fn an_empty_track_has_no_windows() {
        assert!(chunk_ranges(&track(""), 50).is_empty());
    }

    #[test]
    fn windows_prefer_to_end_on_a_sentence() {
        //                        0  1   2    3     4  5    6    7
        let t = track("one two three end. four five six seven");
        let ranges = chunk_ranges(&t, 5);
        assert_eq!(ranges[0], (0, 4), "should break after \"end.\"");
        assert_eq!(ranges.last().unwrap().1, t.len());
    }

    #[test]
    fn a_window_falls_back_to_the_hard_limit_without_a_sentence_end() {
        let t = track("one two three four five six seven eight");
        let ranges = chunk_ranges(&t, 4);
        assert_eq!(ranges, vec![(0, 4), (4, 8)]);
    }

    #[test]
    fn windows_cover_every_word_exactly_once() {
        let t = track(
            "a b c d. e f g h. i j k l m n o p q r. s t u v w x y z",
        );
        for target in [1, 2, 3, 5, 8, 13] {
            let ranges = chunk_ranges(&t, target);
            assert_eq!(ranges.first().unwrap().0, 0, "target {target}");
            assert_eq!(ranges.last().unwrap().1, t.len(), "target {target}");
            for pair in ranges.windows(2) {
                assert_eq!(pair[0].1, pair[1].0, "gap or overlap at target {target}");
            }
            assert!(ranges.iter().all(|(a, b)| a < b), "empty window at {target}");
        }
    }

    #[test]
    fn a_zero_target_does_not_loop_forever() {
        let t = track("one two three");
        let ranges = chunk_ranges(&t, 0);
        assert_eq!(ranges.last().unwrap().1, t.len());
    }

    // -- plans --------------------------------------------------------------

    #[test]
    fn the_unedited_plan_changes_nothing() {
        let t = track("one two three");
        let plan = EditPlan::unedited(&t);
        assert!(plan.is_unedited(&t));
        assert_eq!(plan.text(&t), "one two three");
        assert!(plan.removed(&t).is_empty());
    }

    #[test]
    fn removing_and_restoring_returns_the_original() {
        let t = track("one two three four");
        let mut plan = EditPlan::unedited(&t);
        plan.remove(2);
        assert_eq!(plan.text(&t), "one two four");
        plan.restore(2);
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
    fn moving_outside_the_kept_words_is_an_error() {
        let t = track("one two");
        let mut plan = EditPlan::unedited(&t);
        assert!(plan.move_word(0, 9).is_err());
    }

    #[test]
    fn a_word_kept_twice_is_refused_because_its_audio_exists_once() {
        let t = track("one two three");
        let mut plan = EditPlan::unedited(&t);
        plan.kept.push(1);
        assert_eq!(plan.validate(&t), Err(EditError::DuplicateWord { index: 1 }));
    }

    #[test]
    fn a_plan_cannot_reference_a_word_outside_the_recording() {
        let t = track("one two three");
        let plan = EditPlan { kept: vec![0, 7], max_gap_ms: None, pad_ms: 0 };
        assert_eq!(plan.validate(&t), Err(EditError::IndexOutOfRange { index: 7, len: 3 }));
    }

    #[test]
    fn applying_removals_drops_exactly_those_words() {
        let t = track("one two three four");
        let mut plan = EditPlan::unedited(&t);
        apply_removals(
            &mut plan,
            &[
                Removal { index: 1, reason: RemovalReason::Ai },
                Removal { index: 3, reason: RemovalReason::Manual },
            ],
        );
        assert_eq!(plan.text(&t), "one three");
    }

    #[test]
    fn an_empty_recording_is_handled() {
        let t = track("");
        let plan = EditPlan::unedited(&t);
        assert_eq!(plan.text(&t), "");
        assert_eq!(plan.keep_intervals(&t), vec![]);
        assert!(plan.is_unedited(&t));
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
        assert_eq!(plan.keep_intervals(&t), vec![Interval { start_ms: 0, end_ms: 300 }]);
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
        let t = WordTrack::new(vec![
            Word::new(0, "one", 0, 100),
            Word::new(1, "um", 140, 240),
            Word::new(2, "three", 280, 380),
        ]);
        let plan = EditPlan { kept: vec![0, 2], max_gap_ms: None, pad_ms: 30 };
        let spans = plan.keep_intervals(&t);
        assert_eq!(spans[0].end_ms, 120, "bled into the silence before \"um\"");
        assert_eq!(spans[1].start_ms, 260, "bled into the silence after \"um\"");
        assert!(spans[0].end_ms <= 140 && spans[1].start_ms >= 240);
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
        let t = WordTrack::new(vec![
            Word::new(0, "one", 0, 100),
            Word::new(1, "two", 2100, 2200),
        ]);
        let plan = EditPlan { kept: vec![0, 1], max_gap_ms: Some(200), pad_ms: 0 };
        let spans = plan.keep_intervals(&t);
        assert_eq!(spans.len(), 2, "the pause should split the span");
        assert_eq!((spans[0].end_ms - 100) + (2100 - spans[1].start_ms), 200);
        assert_eq!(plan.output_duration_ms(&t), 400);
    }

    #[test]
    fn a_short_pause_is_left_alone() {
        let t = WordTrack::new(vec![
            Word::new(0, "one", 0, 100),
            Word::new(1, "two", 180, 280),
        ]);
        let plan = EditPlan { kept: vec![0, 1], max_gap_ms: Some(200), pad_ms: 0 };
        assert_eq!(plan.keep_intervals(&t), vec![Interval { start_ms: 0, end_ms: 280 }]);
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

    // -- normalising and serde ----------------------------------------------

    #[test]
    fn normalising_ignores_case_and_punctuation() {
        assert_eq!(normalize("Um,"), "um");
        assert_eq!(normalize("don't"), "dont");
        assert_eq!(normalize("..."), "");
        assert_eq!(normalize("42"), "42");
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

    #[test]
    fn a_renamed_word_field_fails_rather_than_arriving_empty() {
        // The whole point of denying unknown fields here: a Python-side typo in
        // "confidence" would otherwise deserialise to None and silently turn
        // off the editor's highlighting.
        let json = r#"{"index":0,"text":"hi","start_ms":0,"end_ms":100,"confidense":0.9}"#;
        assert!(serde_json::from_str::<Word>(json).is_err());
    }

    #[test]
    fn a_plan_survives_a_round_trip() {
        let plan = EditPlan { kept: vec![0, 2, 5], max_gap_ms: Some(250), pad_ms: 30 };
        let json = serde_json::to_string(&plan).unwrap();
        assert_eq!(serde_json::from_str::<EditPlan>(&json).unwrap(), plan);
    }

    #[test]
    fn a_removal_reason_travels_as_snake_case() {
        assert_eq!(serde_json::to_string(&RemovalReason::Ai).unwrap(), r#""ai""#);
        assert_eq!(serde_json::to_string(&RemovalReason::Manual).unwrap(), r#""manual""#);
    }
}

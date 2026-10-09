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
//! [`apply_edits`] takes a list of find/replace edits and returns the indices
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

    /// Reject a plan that refers to a word this track does not have.
    ///
    /// Invention cannot reach here, because a plan holds indices. What this
    /// catches is a stale plan against a re-transcribed recording.
    ///
    /// A word may appear more than once. Keeping index 7 twice splices the same
    /// piece of audio twice, which is the same voice saying the same word, so
    /// there is nothing to forbid: a speaker who said "Friday" once can be
    /// edited into saying it twice. An earlier version of this rejected that as
    /// a duplicate, which narrowed what an edit could express for no reason
    /// anyone could point at.
    pub fn validate(&self, track: &WordTrack) -> Result<(), EditError> {
        for &i in &self.kept {
            if i as usize >= track.len() {
                return Err(EditError::IndexOutOfRange { index: i, len: track.len() });
            }
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
    PositionOutOfRange { position: usize, len: usize },
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IndexOutOfRange { index, len } => {
                write!(f, "word {index} is outside this recording's {len} words")
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
/// One find/replace edit, the shape an agentic coding tool applies.
///
/// `find` is quoted verbatim from the passage and must match exactly once;
/// `replace` is what it becomes. An empty `replace` deletes the match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub find: String,
    #[serde(default)]
    pub replace: String,
}

/// The reply shape. `edits` is an empty list when nothing needs changing, which
/// has to be distinguishable from a reply nobody could read.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EditList {
    #[serde(default)]
    pub edits: Vec<Edit>,
}

/// Why one edit was refused. The others still apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EditRejection {
    /// `find` had no words in it.
    EmptyFind,
    /// `find` is not in the passage. Usually the model paraphrased its own
    /// quote instead of copying it.
    NotFound { find: String },
    /// `find` matches in more than one place, so applying it would edit a
    /// passage the model did not point at. Refused rather than resolved to the
    /// first hit, and the feedback asks for more surrounding words.
    Ambiguous { find: String, occurrences: usize },
    /// `replace` uses a word that nobody said anywhere in the passage.
    ///
    /// This is the one that protects the product: there is no audio for it.
    /// Note it is checked against the whole passage, not against `find`, because
    /// reusing a word from elsewhere is legitimate.
    NotSpoken { word: String },
    /// Applying it would leave the passage with nothing in it.
    RemovesEverything,
}

impl std::fmt::Display for EditRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyFind => write!(f, "the find text was empty"),
            Self::NotFound { find } => {
                write!(f, "{find:?} does not appear in the passage; quote it exactly")
            }
            Self::Ambiguous { find, occurrences } => write!(
                f,
                "{find:?} appears {occurrences} times; include more of the words around it so it \
                 matches only the one you mean"
            ),
            Self::NotSpoken { word } => write!(
                f,
                "{word:?} was never said, so there is no audio for it; use only words from the \
                 passage"
            ),
            Self::RemovesEverything => write!(f, "that would delete the whole passage"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedEdit {
    /// Which edit in the reply, so feedback can name it.
    pub index: usize,
    pub rejection: EditRejection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditOutcome {
    /// The surviving words, in playback order. Always valid, even when every
    /// edit was refused: then it is the passage unchanged.
    pub kept: Vec<u32>,
    pub applied: usize,
    pub rejected: Vec<RejectedEdit>,
}

/// Apply a list of find/replace edits to one window.
///
/// This is the whole mechanism, and what makes it safe is not a rule the model
/// is asked to follow but the resolution step: every word of `replace` is
/// resolved to an index of a word that was actually spoken, and a word that
/// resolves to nothing is refused. Deleting, reordering and repeating all fall
/// out of that, because all three are just a different sequence of indices.
///
/// Edits apply in order against the result so far, so a later `find` sees what
/// earlier edits produced, exactly as a sequence of patches to a file does. A
/// refused edit changes nothing and the rest still apply.
pub fn apply_edits(window: &[Word], edits: &[Edit]) -> EditOutcome {
    use std::collections::HashMap;

    let mut position_of: HashMap<u32, usize> = HashMap::new();
    for (position, word) in window.iter().enumerate() {
        position_of.insert(word.index, position);
    }
    let normalised: Vec<String> = window.iter().map(|w| normalize(&w.text)).collect();

    let mut kept: Vec<u32> = window.iter().map(|w| w.index).collect();
    let mut applied = 0usize;
    let mut rejected = Vec::new();

    for (ordinal, edit) in edits.iter().enumerate() {
        let needle: Vec<String> =
            edit.find.split_whitespace().map(normalize).filter(|t| !t.is_empty()).collect();
        if needle.is_empty() {
            rejected.push(RejectedEdit { index: ordinal, rejection: EditRejection::EmptyFind });
            continue;
        }

        // Match against the passage as it stands, not as it arrived.
        let hay: Vec<&str> = kept
            .iter()
            .map(|i| position_of.get(i).map(|p| normalised[*p].as_str()).unwrap_or(""))
            .collect();

        let hits: Vec<usize> = if needle.len() > hay.len() {
            Vec::new()
        } else {
            (0..=hay.len() - needle.len())
                .filter(|&at| hay[at..at + needle.len()].iter().zip(&needle).all(|(a, b)| *a == b))
                .collect()
        };

        let at = match hits.len() {
            0 => {
                rejected.push(RejectedEdit {
                    index: ordinal,
                    rejection: EditRejection::NotFound { find: edit.find.trim().to_string() },
                });
                continue;
            }
            1 => hits[0],
            n => {
                rejected.push(RejectedEdit {
                    index: ordinal,
                    rejection: EditRejection::Ambiguous {
                        find: edit.find.trim().to_string(),
                        occurrences: n,
                    },
                });
                continue;
            }
        };
        let span = at..at + needle.len();

        // Resolve each replacement word to a word that was actually spoken.
        let mut resolved: Vec<u32> = Vec::new();
        let mut taken: Vec<bool> = vec![false; span.len()];
        let mut invented: Option<String> = None;

        for token in edit.replace.split_whitespace() {
            let want = normalize(token);
            if want.is_empty() {
                continue;
            }
            // Prefer an unused word from inside the matched span, so an
            // unchanged word keeps its own audio and a reorder stays local.
            let from_span = span
                .clone()
                .enumerate()
                .find(|(offset, position)| {
                    !taken[*offset]
                        && position_of
                            .get(&kept[*position])
                            .is_some_and(|p| normalised[*p] == want)
                });
            if let Some((offset, position)) = from_span {
                taken[offset] = true;
                resolved.push(kept[position]);
                continue;
            }
            // Then any word in the passage, which is what lets a word be
            // repeated or pulled in from nearby.
            match window.iter().position(|w| normalize(&w.text) == want) {
                Some(position) => resolved.push(window[position].index),
                None => {
                    invented = Some(token.to_string());
                    break;
                }
            }
        }

        if let Some(word) = invented {
            rejected
                .push(RejectedEdit { index: ordinal, rejection: EditRejection::NotSpoken { word } });
            continue;
        }

        let mut next = kept.clone();
        next.splice(span, resolved);
        if next.is_empty() {
            rejected.push(RejectedEdit {
                index: ordinal,
                rejection: EditRejection::RemovesEverything,
            });
            continue;
        }
        kept = next;
        applied += 1;
    }

    EditOutcome { kept, applied, rejected }
}

/// Whether an edit keeps any of the passage at all.
pub fn keeps_any_word(window: &[Word], kept: &[u32]) -> bool {
    let _ = window;
    !kept.is_empty()
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

    // -- find/replace edits -------------------------------------------------

    fn edit(find: &str, replace: &str) -> Edit {
        Edit { find: find.into(), replace: replace.into() }
    }

    #[test]
    fn an_empty_replace_deletes_the_match() {
        let t = track("um so I went to the store");
        let out = apply_edits(&t.words, &[edit("um so", "")]);
        assert_eq!(out.applied, 1);
        assert!(out.rejected.is_empty());
        let plan = EditPlan { kept: out.kept, max_gap_ms: None, pad_ms: 0 };
        assert_eq!(plan.text(&t), "I went to the store");
    }

    #[test]
    fn words_can_be_reordered_within_a_match() {
        let t = track("because it was raining we stayed in");
        let out = apply_edits(
            &t.words,
            &[edit("because it was raining we stayed in", "we stayed in because it was raining")],
        );
        assert_eq!(out.applied, 1);
        let plan = EditPlan { kept: out.kept, max_gap_ms: None, pad_ms: 0 };
        assert_eq!(plan.text(&t), "we stayed in because it was raining");
        assert!(is_reordered(&plan.kept));
    }

    #[test]
    fn a_word_can_be_repeated_because_its_audio_can_be_reused() {
        // The speaker said "Friday" once; the edit says it twice. There is
        // audio for both, because it is the same audio.
        let t = track("the deadline is Friday");
        let out = apply_edits(&t.words, &[edit("is Friday", "is Friday Friday")]);
        assert_eq!(out.applied, 1, "{:?}", out.rejected);
        let plan = EditPlan { kept: out.kept, max_gap_ms: None, pad_ms: 0 };
        assert_eq!(plan.text(&t), "the deadline is Friday Friday");
        plan.validate(&t).unwrap();
    }

    #[test]
    fn a_replacement_may_borrow_a_word_from_elsewhere_in_the_passage() {
        // "Friday" is outside the matched span, but it was spoken, so it has
        // audio and may be used.
        let t = track("we ship on Friday and we talk on Monday");
        let out = apply_edits(&t.words, &[edit("we talk on Monday", "we talk on Friday")]);
        assert_eq!(out.applied, 1, "{:?}", out.rejected);
        let plan = EditPlan { kept: out.kept, max_gap_ms: None, pad_ms: 0 };
        assert_eq!(plan.text(&t), "we ship on Friday and we talk on Friday");
    }

    #[test]
    fn a_word_nobody_said_is_refused() {
        // The whole point. There is no audio for it.
        let t = track("the deadline is Friday");
        let out = apply_edits(&t.words, &[edit("is Friday", "is Thursday")]);
        assert_eq!(out.applied, 0);
        assert_eq!(
            out.rejected,
            vec![RejectedEdit {
                index: 0,
                rejection: EditRejection::NotSpoken { word: "Thursday".into() },
            }]
        );
        // And the passage is untouched.
        assert_eq!(out.kept, (0..4).collect::<Vec<u32>>());
    }

    #[test]
    fn an_ambiguous_find_is_refused_with_its_count() {
        // Editing a different "the" changes a different sentence, so this is
        // refused rather than resolved to the first hit. The count is carried
        // so the feedback can ask for more context.
        let t = track("the cat sat on the mat");
        let out = apply_edits(&t.words, &[edit("the", "")]);
        assert_eq!(
            out.rejected,
            vec![RejectedEdit {
                index: 0,
                rejection: EditRejection::Ambiguous { find: "the".into(), occurrences: 2 },
            }]
        );
        assert_eq!(out.applied, 0);
    }

    #[test]
    fn widening_the_quote_resolves_an_ambiguous_one() {
        // Which is exactly what the feedback asks the model to do.
        let t = track("the cat sat on the mat");
        let out = apply_edits(&t.words, &[edit("on the mat", "on a mat")]);
        // "a" was never said, so that still fails; but the LOCATION succeeded.
        assert!(matches!(
            out.rejected.first().map(|r| &r.rejection),
            Some(EditRejection::NotSpoken { .. })
        ));

        let out = apply_edits(&t.words, &[edit("the cat", "cat")]);
        assert_eq!(out.applied, 1, "{:?}", out.rejected);
    }

    #[test]
    fn a_find_that_is_not_there_is_refused() {
        let t = track("the deadline is Friday");
        let out = apply_edits(&t.words, &[edit("the budget is", "")]);
        assert!(matches!(
            out.rejected.first().map(|r| &r.rejection),
            Some(EditRejection::NotFound { .. })
        ));
    }

    #[test]
    fn an_empty_find_is_refused() {
        let t = track("one two three");
        let out = apply_edits(&t.words, &[edit("   ", "two")]);
        assert_eq!(out.rejected[0].rejection, EditRejection::EmptyFind);
    }

    #[test]
    fn edits_apply_in_order_against_the_result_so_far() {
        // A later find sees what earlier edits produced, the way a sequence of
        // patches to a file does.
        let t = track("um so what I wanted to say is the deadline is Friday");
        let out = apply_edits(
            &t.words,
            &[edit("um so", ""), edit("what I wanted to say is", "")],
        );
        assert_eq!(out.applied, 2, "{:?}", out.rejected);
        let plan = EditPlan { kept: out.kept, max_gap_ms: None, pad_ms: 0 };
        assert_eq!(plan.text(&t), "the deadline is Friday");
    }

    #[test]
    fn one_refused_edit_does_not_cost_the_others() {
        // The reason for a patch format rather than a rewritten passage: the
        // blast radius of a bad edit is that edit.
        let t = track("um so I went to the shop and uh it was closed");
        let out = apply_edits(
            &t.words,
            &[
                edit("um so", ""),
                edit("the shop", "the store"),
                edit("and uh it", "and it"),
            ],
        );
        assert_eq!(out.applied, 2);
        assert_eq!(out.rejected.len(), 1);
        assert_eq!(out.rejected[0].index, 1, "the middle edit is the bad one");
        let plan = EditPlan { kept: out.kept, max_gap_ms: None, pad_ms: 0 };
        assert_eq!(plan.text(&t), "I went to the shop and it was closed");
    }

    #[test]
    fn an_edit_that_would_empty_the_passage_is_refused() {
        let t = track("um uh er");
        let out = apply_edits(&t.words, &[edit("um uh er", "")]);
        assert_eq!(out.rejected[0].rejection, EditRejection::RemovesEverything);
        assert_eq!(out.kept.len(), 3, "nothing was taken");
    }

    #[test]
    fn finding_tolerates_punctuation_and_case() {
        let t = track("Um, so... I went to the store");
        let out = apply_edits(&t.words, &[edit("um so", "")]);
        assert_eq!(out.applied, 1, "{:?}", out.rejected);
    }

    #[test]
    fn every_kept_index_belongs_to_the_passage_whatever_the_edits() {
        // The invariant, over a deliberately hostile list.
        let t = track("um so I went to the store and then I went home");
        let hostile = [
            edit("um so", "um so um so"),
            edit("I went", "went I"),
            edit("the store", "the store the store"),
            edit("home", "house"),
            edit("", ""),
            edit("nothing like this", "x"),
        ];
        let out = apply_edits(&t.words, &hostile);
        let plan = EditPlan { kept: out.kept, max_gap_ms: None, pad_ms: 0 };
        plan.validate(&t).unwrap();
        let spoken: Vec<String> = t.words.iter().map(|w| normalize(&w.text)).collect();
        for word in plan.text(&t).split_whitespace() {
            assert!(spoken.contains(&normalize(word)), "{word:?} was never said");
        }
    }

    #[test]
    fn an_empty_edit_list_changes_nothing() {
        let t = track("the quick brown fox");
        let out = apply_edits(&t.words, &[]);
        assert_eq!(out.applied, 0);
        assert!(out.rejected.is_empty());
        assert_eq!(out.kept, (0..4).collect::<Vec<u32>>());
    }

    #[test]
    fn a_window_from_the_middle_of_a_track_keeps_absolute_indices() {
        // Production slices track.words[start..end], so a window's first index
        // is not zero and an off-by-one here cuts the wrong audio.
        let t = track("zero one two three four five six");
        let out = apply_edits(&t.words[3..], &[edit("four", "")]);
        assert_eq!(out.applied, 1, "{:?}", out.rejected);
        assert_eq!(out.kept, vec![3, 5, 6]);
    }

    #[test]
    fn a_reply_parses_from_the_models_json() {
        let json = r#"{"edits":[{"find":"um so","replace":""},{"find":"a b","replace":"b a"}]}"#;
        let list: EditList = serde_json::from_str(json).unwrap();
        assert_eq!(list.edits.len(), 2);
        // An omitted replace is a deletion.
        let list: EditList =
            serde_json::from_str(r#"{"edits":[{"find":"um"}]}"#).unwrap();
        assert_eq!(list.edits[0].replace, "");
        // And no edits at all is a legitimate answer.
        assert_eq!(serde_json::from_str::<EditList>(r#"{"edits":[]}"#).unwrap().edits.len(), 0);
    }

    #[test]
    fn every_rejection_tells_the_model_what_to_do_differently() {
        for r in [
            EditRejection::EmptyFind,
            EditRejection::NotFound { find: "x y".into() },
            EditRejection::Ambiguous { find: "the".into(), occurrences: 3 },
            EditRejection::NotSpoken { word: "Thursday".into() },
            EditRejection::RemovesEverything,
        ] {
            let m = r.to_string();
            assert!(!m.is_empty());
            assert!(!m.contains("EditRejection"), "variant name leaked: {m}");
        }
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

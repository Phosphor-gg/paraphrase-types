# Paraphrase Types

Wire types shared by `paraphrase-backend` and `paraphrase-website`.

## The AI decides what to cut. There are no rules.

**Do not add word lists.** No hesitation list, no filler list, no discourse
marker list, no "obviously removable" heuristics. The model is given a passage
and decides for itself what to remove and what to move.

This has been tried the other way. An earlier version of `src/edit.rs` carried
`HESITATIONS`, `AMBIGUOUS_WORDS` and `AMBIGUOUS_PHRASES`, removed the certain
ones by rule and asked the model only to adjudicate the ambiguous ones. It
scored well on filler removal, and that was exactly the problem: the lists were
a ceiling on recall, not a floor. A rambling aside, a redundant restatement, a
sentence that says nothing, a clause worth moving — none of those are on any
list, so the model was never asked about the things a person most wants gone.

## What makes that safe

`edit::bind_edit` and nothing else. Every word of the model's reply must claim
one not-yet-claimed word from the passage, and the edit is the sequence of
original indices those claims land on.

- Deleting is a word left unclaimed.
- Reordering is claims made out of order.
- A word nobody said has nothing to claim, so the reply is **rejected**.

So "never invent a word" is not an instruction the model is trusted to follow,
and not a validation pass bolted on afterwards. It is a property of the
representation: an `EditPlan` holds indices, and there is no index for a word
that was never spoken.

**An unbindable reply is an error, never a deletion.** This is the single most
important line in this crate. `~/Documents/Github/voice-cleaner` is an earlier
prototype that got it wrong: it diffed the model's prose against the transcript
and cut everything the diff called `replace`, so whenever the model rephrased
instead of cutting, real speech was silently removed and nothing could detect
it. If you find yourself treating an unmatched word as a cut, stop.

## Other things worth knowing

- **One credit is one second of source audio.** There is one model and no
  multiplier, so `credits_for_duration` is nearly an identity. Keep it that way;
  a base rate above 1 turns every minute helper into real arithmetic, which is
  where a sibling product was mispriced twice.
- **Billing is one premium plan with several billing cycles**, like Supervisor,
  with plan detail read from Stripe product metadata. `Tier` is `Free` and
  `Premium`. Do not add a tier ladder.
- **Timestamps on the wire are `String`, ids are `String`.** That is the
  convention the rest of the crate already uses.
- **`asr::TranscribeResponse` sets `deny_unknown_fields`** and
  `tests/fixtures/transcribe_response.json` is the committed contract with
  `paraphrase-audio-api`. Both sides deploy together, so a field renamed in
  Python must fail the request rather than arrive as a silent `None` that
  empties every word timing and makes the product look like it found no speech.
- **`Word::confidence` never decides a cut.** It drives editor highlighting
  only.

## Changing a type

This crate is a **git dependency pinned by `rev`** in every consumer, not a path
dependency: a Docker build has only its own repo as context, so `../paraphrase-types`
cannot resolve there. A change is three steps, not one:

1. commit and push here
2. edit the `rev` in each consumer's `Cargo.toml`
3. `cargo update -p paraphrase-types`

Step 3 is the one people miss. `Cargo.lock` records the full hash, so editing
`Cargo.toml` alone leaves the build on the old tree. Commit `Cargo.toml` and
`Cargo.lock` together, and bump every consumer in the same change: a backend and
a website on different revs both compile while disagreeing about a wire shape,
and that shows up as a runtime deserialisation failure rather than a build
error.

## Tests

CI runs `cargo test`, and that is load bearing rather than hygiene: the edit
invariants, the credit unit and the ASR contract are all enforced by tests, so a
`cargo check`-only workflow would let a mispriced credit or a renamed field
reach main unopposed.

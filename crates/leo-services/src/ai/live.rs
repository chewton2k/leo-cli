//! Live transcription: two independent loops over a recording that is still
//! being written.
//!
//! The rolling loop cuts the tail off the growing WAV every
//! [`ROLL_INTERVAL`] and sends it through the normal transcribe chain, so this
//! works over Groq or a local whisper.cpp with no provider-specific code. The
//! condense loop turns the accumulated raw text into a few readable bullets.
//!
//! Only the scheduling and text-stitching decisions live here, as pure
//! functions; the caller (the TUI's worker thread) performs the effects.

use std::time::Duration;

/// How often a new slice is cut from the growing recording.
///
/// Three seconds, so words appear while the speaker is still talking. It was
/// fifteen, which also meant the *first* text arrived fifteen seconds in — long
/// enough that a user who said one sentence and stopped saw nothing at all and
/// reasonably concluded the feature was broken.
///
/// The cost of a short interval is requests: roughly twenty a minute of
/// continuous speech. Two things keep that in check — silent slices are never
/// sent at all, and a failure backs the interval off ([`backoff`]) rather than
/// hammering a rate-limited provider.
pub const ROLL_INTERVAL: Duration = Duration::from_secs(3);

/// Ceiling for the backoff after repeated failures.
pub const MAX_ROLL_INTERVAL: Duration = Duration::from_secs(24);

/// How loud a slice must be before it is worth transcribing, as a fraction of
/// full scale.
///
/// Digital silence is not the same as a quiet room: a live microphone in a
/// silent room still reads around 0.001, while a muted or unpermitted one reads
/// below 0.0001. This sits between them.
pub const SILENCE_CEILING: f64 = 0.0005;
/// How far back each slice starts before the previous cursor, so a word spoken
/// across a boundary is not cut in half. The overlap is removed from the text
/// again by [`stitch`].
pub const OVERLAP: Duration = Duration::from_secs(1);
/// Longest gap between condense passes.
pub const CONDENSE_INTERVAL: Duration = Duration::from_secs(60);
/// Condense early once this many new words have piled up, so a dense stretch
/// of speech does not sit unread for a full interval.
pub const CONDENSE_WORDS: usize = 400;
/// How much prior context the condense prompt carries, in words. Enough for
/// continuity, small enough to keep the request cheap.
pub const CONTEXT_TAIL_WORDS: usize = 60;
/// How many leading words of a new segment may be treated as repetition.
///
/// Generous on purpose. The nominal overlap is one second — a handful of
/// words — but a slice whose transcription failed leaves the cursor where it
/// was, so the next slice re-covers everything since the last success. That can
/// be a minute of speech, and a bound of "a few words" would let the whole span
/// through twice.
const MAX_OVERLAP_WORDS: usize = 240;
/// How much of the running transcript's tail is compared, in normalized
/// characters. Must be large enough to hold `MAX_OVERLAP_WORDS`; only the end
/// of the transcript can overlap, so bounding it keeps stitching cheap across a
/// long lecture.
const TAIL_COMPARE_CHARS: usize = 1600;
/// Shortest normalized run accepted as a repetition of two or more words.
/// Multi-word agreement is already strong evidence, so this only rules out
/// pairs like "a of".
const MIN_MULTI_WORD_CHARS: usize = 5;
/// Shortest normalized run accepted when only one word matches. Short function
/// words repeat by coincidence constantly ("the", "and", "of"), and dropping
/// one on that basis loses speech, so a lone word must be distinctive.
const MIN_SINGLE_WORD_CHARS: usize = 8;

/// The interval to wait after a failure, doubling up to [`MAX_ROLL_INTERVAL`].
///
/// A three-second cadence is only affordable while requests succeed. Against a
/// rate-limited provider, retrying at the same pace makes the limit permanent,
/// so each failure halves the request rate until it recovers.
pub fn backoff(current: Duration) -> Duration {
    let doubled = current.saturating_mul(2);
    if doubled > MAX_ROLL_INTERVAL {
        MAX_ROLL_INTERVAL
    } else {
        doubled
    }
}

/// Whether a measured peak amplitude means "nothing was said".
///
/// Sending silence to Whisper is worse than sending nothing: it does not return
/// an empty string, it invents plausible filler. "Thank you." and "Thanks for
/// watching!" are its favourites, which is how a user saying "hello, my name
/// is…" into a microphone macOS had muted got back "thank you".
pub fn is_silent(peak_amplitude: f64) -> bool {
    peak_amplitude < SILENCE_CEILING
}

/// Whether a transcription is one of Whisper's silence artifacts rather than
/// speech.
///
/// A second line of defence behind [`is_silent`], for audio quiet enough to
/// hallucinate on but loud enough to pass the level check. Only an exact whole
/// output counts: someone can genuinely say "thank you", but they rarely say it
/// as the entire contents of a slice that contained nothing else.
pub fn is_silence_artifact(text: &str) -> bool {
    const ARTIFACTS: &[&str] = &[
        "thank you",
        "thank you.",
        "thanks for watching",
        "thanks for watching!",
        "thank you for watching",
        "thank you very much",
        "please subscribe",
        "you",
        "bye",
        "bye.",
        ".",
        "[blank_audio]",
        "(silence)",
        "[silence]",
    ];
    let cleaned = text.trim().to_lowercase();
    ARTIFACTS.contains(&cleaned.as_str())
}

/// Where the next slice should start and how long it should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slice {
    pub start_secs: u64,
    pub duration_secs: u64,
}

/// Plan the next slice given how far we have already transcribed and how long
/// the recording currently is.
///
/// Returns `None` when there is not yet enough new audio to be worth a
/// request — a slice shorter than the overlap would be almost entirely
/// material we have already seen.
pub fn next_slice(cursor_secs: u64, recorded_secs: u64) -> Option<Slice> {
    let overlap = OVERLAP.as_secs();
    let min_new = overlap + 1;
    if recorded_secs <= cursor_secs || recorded_secs - cursor_secs < min_new {
        return None;
    }
    let start = cursor_secs.saturating_sub(overlap);
    Some(Slice { start_secs: start, duration_secs: recorded_secs - start })
}

/// Reduce text to comparable characters: letters and digits only, lowercased.
///
/// Spaces and punctuation are dropped rather than normalized, because the same
/// speech can come back tokenized differently across two requests — "breadth
/// first search" one time, "Breadth-first search," the next. Comparing word
/// lists misses that; comparing the character run does not.
fn norm(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// How many leading words of `next_words` repeat the end of `previous_norm`.
///
/// Longest match wins, so a long re-covered span collapses in one step rather
/// than leaving most of it duplicated. Matches shorter than
/// [`MIN_OVERLAP_CHARS`] are ignored as coincidence.
fn repeated_word_count(previous_norm: &str, next_words: &[&str], max_words: usize) -> usize {
    let limit = max_words.min(next_words.len());
    for k in (1..=limit).rev() {
        let candidate = norm(&next_words[..k].join(""));
        let floor = if k == 1 { MIN_SINGLE_WORD_CHARS } else { MIN_MULTI_WORD_CHARS };
        if candidate.len() < floor {
            continue;
        }
        if previous_norm.ends_with(&candidate) {
            return k;
        }
    }
    0
}

/// Append a newly transcribed segment to the running transcript, dropping the
/// part the audio overlap caused to be transcribed twice.
///
/// Whisper is not deterministic across requests, so this cannot assume the
/// repeated words come back identically — comparison is on a normalized
/// character run, and when no overlap is detectable the segment is appended
/// whole. A duplicated phrase reads worse than a missing one, but losing speech
/// is worse than either, so the tie is broken toward keeping text.
pub fn stitch(previous: &str, segment: &str) -> String {
    if segment.trim().is_empty() {
        return previous.to_string();
    }
    if previous.trim().is_empty() {
        return segment.trim().to_string();
    }

    let next_words: Vec<&str> = segment.split_whitespace().collect();
    // Only the tail of the transcript can overlap, and bounding it keeps this
    // cheap as the transcript grows through a long lecture.
    let previous_norm = {
        let n = norm(previous);
        let start = n.len().saturating_sub(TAIL_COMPARE_CHARS);
        n[start..].to_string()
    };

    let dropped = repeated_word_count(&previous_norm, &next_words, MAX_OVERLAP_WORDS);
    let kept = &next_words[dropped..];
    if kept.is_empty() {
        return previous.trim_end().to_string();
    }
    format!("{} {}", previous.trim_end(), kept.join(" "))
}

/// Decide whether it is time to condense.
pub fn should_condense(new_words: usize, since_last: Duration) -> bool {
    new_words > 0 && (new_words >= CONDENSE_WORDS || since_last >= CONDENSE_INTERVAL)
}

/// The last `CONTEXT_TAIL_WORDS` words of what has already been condensed, so
/// the next pass does not repeat itself.
pub fn context_tail(condensed: &str) -> String {
    let words: Vec<&str> = condensed.split_whitespace().collect();
    let start = words.len().saturating_sub(CONTEXT_TAIL_WORDS);
    words[start..].join(" ")
}

/// The prompt for one condense pass. Asks for very little, because this text is
/// read while the user is still listening to something else.
pub fn condense_prompt(new_material: &str, context: &str) -> String {
    format!(
        "You are taking live notes during a lecture. Summarize ONLY the new \
         transcript below into 2-4 short markdown bullets.\n\n\
         Rules:\n\
         - Bullets only, each one line, no preamble and no heading\n\
         - Do not repeat anything already covered in the earlier notes\n\
         - Ignore transcription noise and filler speech\n\
         - Prefer concrete facts, definitions, and action items\n\n\
         Earlier notes (for context, do not repeat):\n{context}\n\n\
         New transcript:\n{new_material}"
    )
}

/// Keep only bullet lines from a condense response, so a chatty model cannot
/// inject a preamble into the live view.
pub fn clean_bullets(response: &str) -> Vec<String> {
    response
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("- ") || l.starts_with("* "))
        .map(|l| format!("- {}", l[2..].trim()))
        .filter(|l| l.len() > 2)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── slicing ─────────────────────────────────────────────────────────────

    /// The bug this guards: the first slice used to be due one full interval
    /// after recording began, so a user who said one sentence and stopped saw
    /// nothing and concluded the feature was broken.
    #[test]
    fn the_roll_interval_is_short_enough_to_feel_live() {
        assert!(
            ROLL_INTERVAL <= Duration::from_secs(5),
            "a {}s cadence is not live",
            ROLL_INTERVAL.as_secs()
        );
    }

    #[test]
    fn backoff_doubles_and_then_stops_at_the_ceiling() {
        assert_eq!(backoff(Duration::from_secs(3)), Duration::from_secs(6));
        assert_eq!(backoff(Duration::from_secs(6)), Duration::from_secs(12));
        assert_eq!(backoff(MAX_ROLL_INTERVAL), MAX_ROLL_INTERVAL);
        // And it never runs away, however many failures pile up.
        let mut interval = ROLL_INTERVAL;
        for _ in 0..50 {
            interval = backoff(interval);
        }
        assert_eq!(interval, MAX_ROLL_INTERVAL);
    }

    /// Real measurements: a muted or unpermitted microphone reads at digital
    /// silence, a quiet room reads above it, and speech is far above.
    #[test]
    fn silence_is_told_apart_from_a_quiet_room_and_from_speech() {
        // Measured from a macOS mic with permission denied, and from a
        // synthesised silent WAV — both 0.000031.
        assert!(is_silent(0.000031), "digital silence read as sound");
        assert!(is_silent(0.0));
        // A live mic in a quiet room.
        assert!(!is_silent(0.002), "a quiet room read as silence");
        // Measured from `say` output.
        assert!(!is_silent(0.784), "speech read as silence");
    }

    /// Whisper's invented filler must not reach the transcript.
    #[test]
    fn whispers_silence_artifacts_are_recognized() {
        for artifact in [
            "Thank you.",
            "thank you",
            "  Thanks for watching! ",
            "[BLANK_AUDIO]",
            ".",
            "you",
        ] {
            assert!(is_silence_artifact(artifact), "missed {artifact:?}");
        }
    }

    /// And real speech must survive, including a sentence that merely contains
    /// one of those phrases.
    #[test]
    fn real_speech_is_not_mistaken_for_an_artifact() {
        for speech in [
            "Hello, my name is Charlton",
            "thank you for the explanation, that makes sense",
            "So thank you. Next, the ownership rules.",
            "Thanks for watching the lecture recording I made yesterday",
        ] {
            assert!(!is_silence_artifact(speech), "dropped real speech: {speech:?}");
        }
    }

    #[test]
    fn the_first_slice_starts_at_zero() {
        let s = next_slice(0, 15).unwrap();
        assert_eq!(s.start_secs, 0);
        assert_eq!(s.duration_secs, 15);
    }

    #[test]
    fn later_slices_reach_back_by_the_overlap() {
        let s = next_slice(15, 30).unwrap();
        assert_eq!(s.start_secs, 14, "one second of back-overlap");
        assert_eq!(s.duration_secs, 16);
        assert_eq!(s.start_secs + s.duration_secs, 30, "covers up to now");
    }

    #[test]
    fn no_slice_until_there_is_new_audio_worth_sending() {
        assert_eq!(next_slice(15, 15), None);
        assert_eq!(next_slice(15, 16), None, "one second is all overlap");
        assert!(next_slice(15, 17).is_some());
    }

    /// A recording that somehow reports shorter than the cursor must not
    /// produce a negative or wrapped duration.
    #[test]
    fn a_shrinking_recording_yields_no_slice() {
        assert_eq!(next_slice(30, 10), None);
        assert_eq!(next_slice(0, 0), None);
    }

    // ── stitching ───────────────────────────────────────────────────────────

    #[test]
    fn the_first_segment_becomes_the_transcript() {
        assert_eq!(stitch("", "hello there"), "hello there");
        assert_eq!(stitch("   ", "  hello there "), "hello there");
    }

    #[test]
    fn an_empty_segment_changes_nothing() {
        assert_eq!(stitch("hello there", ""), "hello there");
        assert_eq!(stitch("hello there", "   "), "hello there");
    }

    #[test]
    fn overlapping_words_are_dropped_once() {
        let previous = "the mitochondria is the powerhouse of the";
        let segment = "of the cell and it makes ATP";
        assert_eq!(
            stitch(previous, segment),
            "the mitochondria is the powerhouse of the cell and it makes ATP"
        );
    }

    /// Whisper re-punctuates and re-capitalizes across requests, so the
    /// repeated words rarely come back byte-identical.
    #[test]
    fn overlap_is_detected_despite_punctuation_and_case_changes() {
        let previous = "we will now discuss breadth first search";
        let segment = "Breadth-first search, explores level by level";
        let out = stitch(previous, segment);
        assert_eq!(
            out, "we will now discuss breadth first search explores level by level",
            "got: {out}"
        );
    }

    #[test]
    fn a_segment_entirely_repeating_the_tail_adds_nothing() {
        let previous = "one two three four";
        assert_eq!(stitch(previous, "three four"), "one two three four");
    }

    #[test]
    fn unrelated_text_is_appended_whole() {
        // Losing speech is worse than a duplicated phrase, so no overlap means
        // keep everything.
        let out = stitch("first topic done", "completely different words here");
        assert_eq!(out, "first topic done completely different words here");
    }

    /// A slice whose transcription failed leaves the cursor put, so the next
    /// slice re-covers everything since the last success. That span must
    /// collapse, not appear twice.
    #[test]
    fn a_long_re_covered_span_is_deduplicated_whole() {
        let previous = "breadth first search explores a graph level by level using a queue \
                        depth first search uses a stack instead and goes as deep as possible \
                        before backtracking";
        let segment = "uses a stack instead and goes as deep as possible before backtracking. \
                       Both run in linear time.";
        let out = stitch(previous, segment);
        assert_eq!(
            out.matches("uses a stack instead").count(),
            1,
            "the re-covered span was duplicated: {out}"
        );
        assert!(out.ends_with("Both run in linear time."), "got: {out}");
    }

    /// A lone short function word matching is coincidence, not repetition.
    #[test]
    fn a_single_short_word_is_not_treated_as_overlap() {
        let out = stitch("we discussed the", "the queue holds vertices");
        assert_eq!(out, "we discussed the the queue holds vertices");
    }

    /// A lone distinctive word is a real overlap.
    #[test]
    fn a_single_long_word_is_treated_as_overlap() {
        let out = stitch("it finishes by backtracking", "backtracking to the previous vertex");
        assert_eq!(out, "it finishes by backtracking to the previous vertex");
    }

    #[test]
    fn a_long_coincidental_repeat_is_not_treated_as_overlap() {
        // "the" appearing in both is not an overlap worth trusting beyond the
        // bounded window, and the result must never lose the new sentence.
        let previous = "a b c d e f g h i j k l m n o p the";
        let segment = "the second half of the lecture starts now";
        let out = stitch(previous, segment);
        assert!(out.ends_with("second half of the lecture starts now"), "got: {out}");
        assert!(out.starts_with("a b c"), "got: {out}");
    }

    #[test]
    fn stitching_many_segments_in_sequence_stays_readable() {
        let segments = [
            "today we cover graphs",
            "cover graphs and their traversals",
            "traversals like BFS and DFS",
            "and DFS which uses a stack",
        ];
        let mut transcript = String::new();
        for s in segments {
            transcript = stitch(&transcript, s);
        }
        assert_eq!(
            transcript,
            "today we cover graphs and their traversals like BFS and DFS which uses a stack"
        );
    }

    // ── condense scheduling ─────────────────────────────────────────────────

    #[test]
    fn condensing_waits_for_either_enough_words_or_enough_time() {
        assert!(!should_condense(10, Duration::from_secs(5)));
        assert!(should_condense(10, CONDENSE_INTERVAL));
        assert!(should_condense(CONDENSE_WORDS, Duration::from_secs(1)));
    }

    /// A silent stretch must not fire an empty condense request every minute.
    #[test]
    fn no_new_words_never_condenses() {
        assert!(!should_condense(0, Duration::from_secs(600)));
    }

    #[test]
    fn the_context_tail_is_bounded() {
        let long = (0..500).map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
        let tail = context_tail(&long);
        assert_eq!(tail.split_whitespace().count(), CONTEXT_TAIL_WORDS);
        assert!(tail.ends_with("499"), "the tail is the most recent words");
        assert_eq!(context_tail(""), "");
    }

    #[test]
    fn the_condense_prompt_carries_both_halves_and_asks_for_bullets() {
        let p = condense_prompt("new speech here", "earlier bullets");
        assert!(p.contains("new speech here"));
        assert!(p.contains("earlier bullets"));
        assert!(p.contains("2-4"));
    }

    // ── response cleaning ───────────────────────────────────────────────────

    #[test]
    fn only_bullet_lines_survive() {
        let response = "Sure! Here are the notes:\n\n- first point\n* second point\n\nLet me know!";
        assert_eq!(
            clean_bullets(response),
            vec!["- first point".to_string(), "- second point".to_string()]
        );
    }

    #[test]
    fn a_response_with_no_bullets_yields_nothing_rather_than_prose() {
        assert!(clean_bullets("I could not hear anything useful.").is_empty());
        assert!(clean_bullets("").is_empty());
        // A bullet marker with no content is dropped too.
        assert!(clean_bullets("- \n-  ").is_empty());
    }
}

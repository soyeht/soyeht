//! The numbers the turn machine runs on (`x-clientPolicy.voice`).

use soyeht_protocol::client_policy::{
    VOICE_BARGE_IN_MS, VOICE_CONTINUERS, VOICE_END_THRESHOLD, VOICE_FINALIZE_TIMEOUT_MS,
    VOICE_FRAME_MS, VOICE_HALF_DUPLEX_TAIL_MS, VOICE_HISTORY_MS, VOICE_INCOMPLETE_SILENCE_MS,
    VOICE_PRE_ROLL_MS, VOICE_PROBE_TIMEOUT_MS, VOICE_SILENCE_MS, VOICE_SPEECH_START_MS,
    VOICE_START_THRESHOLD,
};

/// A time of the policy, in milliseconds. The values are a few seconds at most.
fn ms(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The numbers the turn machine runs on, in whole milliseconds. The defaults are the policy's, the
/// same numbers the Apple clients read; a case of `contracts/vectors/voice-turns.json` may
/// override any of them.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceTurnConfig {
    /// The length of one voice-activity frame.
    pub frame_ms: i64,
    /// A frame is voiced from this speech probability up, or from `end_threshold` up while the
    /// frame before it was voiced.
    pub start_threshold: f64,
    pub end_threshold: f64,
    /// How long voiced frames must run, without a break, to count as speech.
    pub speech_start_ms: i64,
    /// Audio kept from before the start was noticed, so the first word is not clipped.
    pub pre_roll_ms: i64,
    /// How far back the microphone's audio is still kept.
    pub history_ms: i64,
    /// Silence after speech before the end-of-turn model is asked.
    pub silence_ms: i64,
    /// The turn ends after this much silence, whatever the model says or whether it answered.
    pub incomplete_silence_ms: i64,
    /// The recognizer has this long, after the end of a turn, to give its final text.
    pub finalize_timeout_ms: i64,
    /// How long the person must talk over the avatar, or over the wait for its reply, before it
    /// is worth hearing what they say.
    pub barge_in_ms: i64,
    /// A listen for an interruption that decided nothing in this time is dropped.
    pub probe_timeout_ms: i64,
    /// The microphone is not heard while the avatar speaks; only an explicit interrupt stops it.
    pub half_duplex: bool,
    /// In half duplex, the microphone stays deaf this long after the avatar's voice ends.
    pub half_duplex_tail_ms: i64,
    /// Short replies of a listener, which do not take the turn from the avatar.
    pub continuers: Vec<String>,
}

impl Default for VoiceTurnConfig {
    fn default() -> Self {
        Self {
            frame_ms: ms(VOICE_FRAME_MS),
            start_threshold: VOICE_START_THRESHOLD,
            end_threshold: VOICE_END_THRESHOLD,
            speech_start_ms: ms(VOICE_SPEECH_START_MS),
            pre_roll_ms: ms(VOICE_PRE_ROLL_MS),
            history_ms: ms(VOICE_HISTORY_MS),
            silence_ms: ms(VOICE_SILENCE_MS),
            incomplete_silence_ms: ms(VOICE_INCOMPLETE_SILENCE_MS),
            finalize_timeout_ms: ms(VOICE_FINALIZE_TIMEOUT_MS),
            barge_in_ms: ms(VOICE_BARGE_IN_MS),
            probe_timeout_ms: ms(VOICE_PROBE_TIMEOUT_MS),
            half_duplex: false,
            half_duplex_tail_ms: ms(VOICE_HALF_DUPLEX_TAIL_MS),
            continuers: VOICE_CONTINUERS.split('|').map(str::to_owned).collect(),
        }
    }
}

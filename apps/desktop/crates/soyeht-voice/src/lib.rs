//! The pure rules of the voice loop, as the Windows and Linux app implements them (ADR 0166,
//! slice 8a; contract 20, `docs/contracts/20-voice-loop.md`): the turn machine, the speech text of
//! a reply, the sentence chunker, the reply feed and the part of a reply the person heard.
//!
//! Nothing here records, plays, recognises, speaks or keeps a clock. Time is data: the stamp of
//! each input, in milliseconds on the microphone's clock. Every number is `x-clientPolicy.voice`
//! (`soyeht_protocol::client_policy::VOICE_...`). The rules are the Apple clients' too, and both
//! pass the same vector files, `contracts/vectors/voice-turns.json` and
//! `contracts/vectors/speech-text.json`.
//!
//! The speech text reads what the shared Markdown parser of `soyeht-client` (ADR 0087) gives, so
//! this crate sits with the desktop client and not with the engine's building blocks.

mod chunker;
mod config;
mod heard;
mod held_back;
mod interruption;
mod machine;
mod reply_feed;
mod speech_text;
mod text;

pub use chunker::SentenceChunker;
pub use config::VoiceTurnConfig;
pub use heard::{WordTiming, heard_text};
pub use machine::{VoiceTurnAction, VoiceTurnInput, VoiceTurnMachine, VoiceTurnPhase};
pub use reply_feed::{ReplyFeed, ReplyUpdate};
pub use speech_text::speakable;

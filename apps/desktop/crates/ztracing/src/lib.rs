//! The part of `ztracing` that GPUI's `sum_tree` uses: the `#[instrument]` attribute.
//!
//! Zed's own `ztracing`, `ztracing_macro` and `zlog` crates are licensed GPL-3.0-or-later at the
//! revision of GPUI that this workspace pins, and Soyeht is Apache-2.0 (ADR 0149). The workspace
//! replaces `ztracing` with this crate through `[patch]`, so neither `ztracing_macro` nor `zlog` is
//! built or linked. `sum_tree` writes `#[instrument(skip_all)]` on a few functions and nothing else
//! from `ztracing`; Soyeht does not profile with Tracy, so the attribute leaves the function as it
//! is. This was written from that use alone.

use proc_macro::TokenStream;

/// Accepts the arguments of a tracing span and returns the function unchanged.
#[proc_macro_attribute]
pub fn instrument(_arguments: TokenStream, function: TokenStream) -> TokenStream {
    function
}

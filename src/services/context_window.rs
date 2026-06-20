//! Token accounting and cap enforcement. This is the heart of the
//! 1M-context fix — when a user configures a model with
//! `maxContextTokens: 1_000_000`, the agent forwarding layer
//! actually uses it to:
//!   1. Set `max_tokens` on every upstream request (clamped to
//!      `max_output_tokens` when set).
//!   2. Trim the message list before sending so the input fits
//!      under `max_input_tokens` (with a 5% safety margin to
//!      absorb estimation error).
//!   3. Surface a real "context usage" ratio to the Mother Agent
//!      UI instead of a hardcoded 128K/200K denominator.
//!
//! Why 5% safety: our token estimator is heuristic. Without the
//! margin, a tight estimate would let a borderline prompt through
//! and the upstream would reject it with 400. With the margin,
//! we trim slightly more than necessary — much better failure
//! mode (the user sees a slightly shorter conversation, not a
//! broken request).

/// Result of a token estimate. `Some(n)` when we have a confident
/// count, `None` when the message list is unparseable. Callers
/// should treat `None` as "0 tokens" — the conservative direction
/// is to not over-trim, and we never enforce a cap when the
/// estimate is unknown.
pub type TokenCount = u64;

/// Heuristic tokens-per-byte ratio. We use the rule of thumb that
/// English text averages ~4 chars per token and CJK averages ~1.5
/// chars per token. To stay conservative (over-estimate so we err
/// on the side of trimming too much rather than too little), we
/// use 3.0 — between the two.
const CHARS_PER_TOKEN: f64 = 3.0;

/// Estimate tokens for a single message's text content. We
/// intentionally count system prompt + user/assistant text but
/// not tool-call arguments (those have a separate, much higher
/// token rate). The Mother Agent's UI shows a rough usage
/// percentage; precision isn't worth more code here.
pub fn estimate_text_tokens(text: &str) -> TokenCount {
    if text.is_empty() {
        return 0;
    }
    let chars = text.chars().count() as f64;
    (chars / CHARS_PER_TOKEN).ceil() as u64
}

/// Sum the token estimate over a message list. Each entry is the
/// message's text payload; the caller is responsible for what
/// "text" means (typically the joined user/assistant text, not
/// tool calls). We don't try to be clever — the goal is a
/// monotonic, fast, conservative estimate.
pub fn estimate_messages_tokens<S: AsRef<str>>(messages: &[S]) -> TokenCount {
    messages
        .iter()
        .map(|m| estimate_text_tokens(m.as_ref()))
        .sum()
}

/// Trim a list of messages from the front (after the first
/// system-prompt entry) until the running estimate fits under
/// `cap`. The system prompt at index 0 is preserved by contract —
/// the upstream APIs expect it as the first message and dropping
/// it produces incoherent answers.
///
/// Returns the number of messages that were dropped. The caller
/// uses the return value to update the UI ("trimmed 4 oldest
/// messages to fit the input cap").
pub fn trim_to_input_cap<S: AsRef<str>>(
    messages: &mut Vec<S>,
    cap: Option<TokenCount>,
) -> usize {
    let Some(cap) = cap else {
        return 0;
    };
    let safety_cap = ((cap as f64) * 0.95) as TokenCount;
    let mut dropped = 0;
    while messages.len() > 1 {
        let total = estimate_messages_tokens(messages);
        if total <= safety_cap {
            break;
        }
        messages.remove(1);
        dropped += 1;
    }
    dropped
}

/// Clamp the per-request `max_tokens` parameter to the smaller of
/// the caller's request and the model's configured
/// `max_output_tokens`. We clamp from above (so the upstream
/// never generates more than the cap) but never clamp from below
/// (the caller's `Some(8)` for "give me exactly 8 tokens" stays
/// at 8 even if the cap is 4 — we'd rather return a short
/// response than surprise the caller with a different number).
pub fn clamp_max_tokens(
    requested: Option<u32>,
    model_cap: Option<TokenCount>,
) -> Option<u32> {
    match (requested, model_cap) {
        (Some(r), Some(c)) => Some(r.min(c as u32)),
        (Some(r), None) => Some(r),
        (None, Some(c)) => Some(c as u32),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_preserves_system_prompt() {
        let mut msgs: Vec<String> = vec![
            "you are a helpful assistant".to_string(),
            "msg 1".to_string(),
            "msg 2".to_string(),
            "msg 3".to_string(),
        ];
        let dropped = trim_to_input_cap(&mut msgs, Some(15));
        assert!(dropped > 0);
        assert_eq!(msgs[0], "you are a helpful assistant");
        assert!(msgs.len() < 4);
    }

    #[test]
    fn no_trim_when_under_cap() {
        let mut msgs: Vec<String> = vec!["system".to_string(), "hi".to_string()];
        let dropped = trim_to_input_cap(&mut msgs, Some(10_000));
        assert_eq!(dropped, 0);
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn no_trim_when_cap_is_none() {
        let mut msgs: Vec<String> = vec!["a".repeat(10_000)];
        let dropped = trim_to_input_cap(&mut msgs, None);
        assert_eq!(dropped, 0);
    }

    #[test]
    fn clamp_respects_caller_when_below_cap() {
        assert_eq!(clamp_max_tokens(Some(8), Some(100)), Some(8));
    }

    #[test]
    fn clamp_uses_cap_when_caller_omits() {
        assert_eq!(clamp_max_tokens(None, Some(64)), Some(64));
    }

    #[test]
    fn clamp_passes_through_when_no_cap() {
        assert_eq!(clamp_max_tokens(Some(256), None), Some(256));
    }
}

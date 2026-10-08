//! Reads the claims of a JSON Web Token without verifying it: the token is only used to find
//! the account id and expiry the provider's own client stored next to it.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;

pub(crate) fn claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The `exp` claim, in Unix seconds.
pub(crate) fn expiry(token: &str) -> Option<i64> {
    claims(token)?.get("exp")?.as_i64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_claims_and_expiry_from_the_payload() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"exp":1800000000,"sub":"a|b"}"#);
        let token = format!("header.{payload}.signature");
        assert_eq!(expiry(&token), Some(1_800_000_000));
        assert_eq!(claims(&token).unwrap()["sub"], "a|b");
        assert!(claims("not-a-token").is_none());
    }
}

//! The approval code of a pairing on Linux (ADR 0164): six decimal digits that the pairing device
//! shows and the person types at the computer. The rule for reading what the person typed is here
//! so that every engine reads it the same way; the cases are
//! `contracts/vectors/pairing-approval.json`.

/// How many digits a code has.
pub const DIGITS: usize = 6;

/// The digits of `text` as ASCII bytes when it is a code, `None` otherwise. Only the ASCII digits
/// and the ASCII space count and the blanks are ignored; any other character, and any count of
/// digits but six, is not a code.
pub fn read(text: &str) -> Option<[u8; DIGITS]> {
    let mut digits = [0u8; DIGITS];
    let mut count = 0;
    for byte in text.bytes() {
        match byte {
            b' ' => {}
            b'0'..=b'9' if count < DIGITS => {
                digits[count] = byte;
                count += 1;
            }
            _ => return None,
        }
    }
    (count == DIGITS).then_some(digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_code_is_read_as_the_shared_vector_says() {
        let vector: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/vectors/pairing-approval.json"
        ))
        .unwrap();
        let cases = vector["cases"].as_array().unwrap();
        assert!(cases.len() >= 10);
        for case in cases {
            let found = read(case["input"].as_str().unwrap());
            let found = found.map(|digits| String::from_utf8(digits.to_vec()).unwrap());
            assert_eq!(
                found.as_deref(),
                case["digits"].as_str(),
                "{}",
                case["name"]
            );
            // The grouping a device shows is three digits, a space and three digits.
            if let Some(digits) = case["digits"].as_str() {
                let grouped = format!("{} {}", &digits[..3], &digits[3..]);
                assert_eq!(Some(grouped.as_str()), case["grouped"].as_str());
            }
        }
    }
}

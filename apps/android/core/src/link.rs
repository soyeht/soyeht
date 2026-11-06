//! What the phone learns from a pairing link before it pairs: the same parse `Core::pair` runs, so a
//! link that reaches the app by a tap, by the paste field or by the camera is accepted or refused by
//! one rule (`soyeht_protocol::pairing_link`).

use crate::CoreError;
use soyeht_client::{Error, device};

/// The computer a link names.
#[derive(uniffi::Record)]
pub struct PairingOffer {
    /// `host:port`, with an IPv6 host in brackets: the address the phone would connect to.
    pub address: String,
}

/// Reads a pairing link without connecting to anything. A text the parser refuses (another product's
/// link, a link of an engine too old to prove itself, an address outside loopback and the tailnet) is
/// refused in the words the person reads.
#[uniffi::export]
pub fn pairing_offer(link: String) -> Result<PairingOffer, CoreError> {
    let link = device::parse_link(&link).map_err(Error::Pairing)?;
    Ok(PairingOffer {
        address: link.address.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::pairing_link;

    /// A link written by the function the engine writes it with. The key must be a point on the
    /// curve: this one is the engine key of the connection tests.
    fn link(address: &str) -> String {
        let key = "0EqyMnQrtKs6E2i9RhXk5tAiSrcaAWuvhSCjMsl3hzc";
        pairing_link::uri(
            "6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b",
            address,
            &"A".repeat(soyeht_protocol::limits::PAIRING_CODE_CHARS),
            key,
        )
    }

    #[test]
    fn a_link_the_parser_takes_names_its_address() {
        let offer = pairing_offer(link("100.101.102.103:7443")).unwrap();
        assert_eq!(offer.address, "100.101.102.103:7443");
        let offer = pairing_offer(format!("  {}\n", link("127.0.0.1:9000"))).unwrap();
        assert_eq!(offer.address, "127.0.0.1:9000");
    }

    #[test]
    fn what_the_parser_refuses_is_refused_in_words() {
        let refused = [
            "https://example.com".to_owned(),
            String::new(),
            "soyeht-next://pair?address=100.64.0.1:1&code=x&engineId=y".to_owned(),
            link("192.168.1.5:7443"),
            link("8.8.8.8:7443"),
            link("100.101.102.103:0"),
            link("100.101.102.103:7443").replace("engineKey=", "engineKey=x"),
        ];
        for text in refused {
            assert!(
                matches!(pairing_offer(text.clone()), Err(CoreError::Failed { .. })),
                "{text}"
            );
        }
    }

    #[test]
    fn a_link_of_an_engine_that_cannot_prove_itself_asks_to_update_the_computer() {
        let old = "soyeht-next://pair?engineId=6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b\
                   &address=100.101.102.103:7443&code=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        assert!(matches!(
            pairing_offer(old.to_owned()),
            Err(CoreError::Failed { reason }) if reason.contains("Update Soyeht")
        ));
    }
}

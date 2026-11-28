//! Bounded sparse frames around the private console's positional Serde contracts.
use ic_wire::{Encoding, Error, Limits, evolution};
use serde::{Deserialize, Serialize};

/// Encode the fixture body with an empty root extension block.
/// # Errors
/// Returns the Wire encoding or budget refusal.
pub fn encode<T: Serialize + ?Sized>(
    value: &T,
    encoding: Encoding,
    limits: Limits,
) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    evolution::encode_projected_into(&mut bytes, encoding, limits, |_| {
        Ok(evolution::Projected::empty(value))
    })?;
    Ok(bytes)
}

/// Decode the positional body and validate any unrecognized extension fields.
/// # Errors
/// Returns the Wire framing, decoding or budget refusal.
pub fn decode<'de, T: Deserialize<'de>>(
    bytes: &'de [u8],
    encoding: Encoding,
    limits: Limits,
) -> Result<T, Error> {
    evolution::decode_projected(bytes, encoding, limits, |message, context| {
        message.extensions.visit(context, |_, _| Ok(()))?;
        Ok(message.core)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_projection_accepts_optional_extensions_and_checks_their_headers() {
        let limits = Limits {
            bytes: 64,
            nodes: 64,
            depth: 8,
            allocation: 1024,
        };
        let payload = Encoding::Payload;
        let body = 7_u8;
        assert_eq!(
            encode(&body, payload, limits),
            ic_wire::encode(&ic_wire::Message(body), payload, limits)
        );
        assert_eq!(decode::<u8>(&[7, 3, 1, 1, 9], payload, limits), Ok(body));
        assert!(decode::<u8>(&[7, 3, 0, 1, 9], payload, limits).is_err());
        assert!(decode::<u8>(&[7], payload, limits).is_err());
    }
}

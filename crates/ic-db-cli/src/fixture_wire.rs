//! Bounded sparse frames around the private console's positional Serde contracts.
use canic::wire::evolution;
pub use canic::wire::{
    ByteBuf, Direction, Encoding, Error, LimitKind, Limits, encode, validate_request_header,
};
use serde::Deserialize;

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
            allocation: 8192,
        };
        let payload = Encoding::Payload;
        let body = 7_u8;
        assert_eq!(encode(&body, payload, limits), Ok(vec![body, 0]));
        assert_eq!(decode::<u8>(&[7, 3, 1, 1, 9], payload, limits), Ok(body));
        assert!(decode::<u8>(&[7, 3, 0, 1, 9], payload, limits).is_err());
        assert!(decode::<u8>(&[7], payload, limits).is_err());
    }
}

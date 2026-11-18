use super::*;
use crate::parse_console_line;

#[test]
fn listing_bounds_refuse_before_calls() {
    let base = "witness-list --provider http://localhost:8000 --canister aaaaa-aa --anonymous";
    assert!(parse_console_line(base).is_ok());
    for option in [
        "--limit 0",
        "--limit 4",
        "--work 0",
        "--work 5",
        "--after 42",
        "--entity Item",
        "--minimum -1",
    ] {
        assert!(parse_console_line(&format!("{base} {option}")).is_err());
    }
}

#[test]
fn listing_reply_is_bounded_and_empty_pages_keep_continuation() {
    // Independent producer-shaped tuples; no client output DTO used for encoding.
    let frame = |rows: Vec<(u64, u64)>, next: Option<Vec<u8>>, examined: u32, method| {
        canic_wire::encode(
            &Ok::<_, u8>((rows, next, examined)),
            Encoding::Frame {
                method,
                direction: Direction::Response,
            },
            RPC_LIMITS,
        )
        .unwrap()
    };
    let bytes = frame(vec![], Some(vec![9; 35]), 1, QUERY);
    let page = decode(&bytes, 1, 1).unwrap();
    assert!(page.rows.is_empty());
    assert_eq!(page.next, Some(vec![9; 35]));
    assert!(decode(&frame(vec![(1, 10), (2, 20)], None, 2, QUERY), 1, 2).is_err());
    assert!(decode(&frame(vec![], None, 2, QUERY), 1, 1).is_err());
    assert!(decode(&frame(vec![(1, 10)], None, 0, QUERY), 1, 1).is_err());
    assert!(decode(&frame(vec![], Some(vec![]), 1, QUERY), 1, 1).is_err());
    assert!(decode(&frame(vec![], None, 0, QUERY + 1), 1, 1).is_err());
    assert!(decode(&bytes[..bytes.len() - 1], 1, 1).is_err());
    assert!(decode(&vec![0; RPC_LIMITS.bytes + 1], 1, 1).is_err());
}

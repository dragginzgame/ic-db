use super::{Command, FilterArgs, charged_cycles, parse_console_line};

#[test]
fn named_payload_filters_keep_their_wire_positions() {
    let filters = super::PayloadFilters {
        minimum: Some(vec![]),
        maximum: None,
        prefix: Some(vec![1]),
        suffix: Some(vec![2]),
        contains: Some(vec![3]),
    };
    let previous = [
        Some(vec![]),
        None,
        Some(vec![1_u8]),
        Some(vec![2]),
        Some(vec![3]),
    ];
    let frame = || ic_wire::Encoding::Frame {
        method: 3,
        direction: ic_wire::Direction::Request,
    };
    assert_eq!(
        ic_wire::encode(&filters, frame(), super::LIMITS).unwrap(),
        ic_wire::encode(&previous, frame(), super::LIMITS).unwrap(),
    );
}

#[test]
fn query_cursor_argument_checks_framing_before_execution() {
    let tail = "00".repeat(33);
    assert!(parse_console_line(&format!("query Entry --after 0100{tail}")).is_ok());
    for prefix in ["0200", "0103"] {
        let error = parse_console_line(&format!("query Entry --after {prefix}{tail}"))
            .err()
            .unwrap();
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }
}

#[test]
fn blob_literals_are_exact_bounded_bytes() {
    for (literal, expected) in [
        ("0x", vec![]),
        ("0x00fF80", vec![0, 255, 128]),
        ("0x00", vec![0]),
    ] {
        let cli =
            parse_console_line(&format!("query BinaryObject --payload-min {literal}")).unwrap();
        assert!(
            matches!(cli.command, Some(Command::Query {filters: FilterArgs {payload_min:Some(value),..},..}) if value.0==expected)
        );
    }
    let max = format!("0x{}", "ff".repeat(1024));
    assert!(parse_console_line(&format!("query BinaryObject --payload-max {max}")).is_ok());
    for literal in [
        "00ff".to_owned(),
        "0Xff".into(),
        "0xf".into(),
        "0xzz".into(),
        "0xé".into(),
        "0x００".into(),
        format!("{max}00"),
    ] {
        assert!(
            parse_console_line(&format!("query BinaryObject --payload-min {literal}")).is_err()
        );
    }
    assert!(parse_console_line("count BinaryObject").is_ok());
    assert!(parse_console_line("aggregate BinaryObject --select min:payload").is_ok());
    let cli =
        parse_console_line("aggregate BinaryObject --payload-min 0x --payload-max 0xFF").unwrap();
    assert!(matches!(cli.command, Some(Command::Aggregate {
        filters: FilterArgs { payload_min: Some(lower), payload_max: Some(upper), .. }, ..
    }) if lower.0.is_empty() && upper.0 == [255]));
    for command in ["query", "aggregate"] {
        let cli = parse_console_line(&format!(
            "{command} BinaryObject --payload-prefix 0x00 --payload-suffix 0xfF80"
        ))
        .unwrap();
        let suffix = match cli.command.unwrap() {
            Command::Query { filters, .. } | Command::Aggregate { filters, .. } => {
                filters.payload_suffix.unwrap()
            }
            _ => panic!("wrong command"),
        };
        assert_eq!(suffix.0, [255, 128]);
        for literal in ["0x0".to_owned(), format!("{max}00")] {
            assert!(
                parse_console_line(&format!(
                    "{command} BinaryObject --payload-suffix {literal}"
                ))
                .is_err()
            );
        }
        let cli =
            parse_console_line(&format!("{command} BinaryObject --payload-prefix 0x00fF")).unwrap();
        let prefix = match cli.command.unwrap() {
            Command::Query { filters, .. } | Command::Aggregate { filters, .. } => {
                filters.payload_prefix.unwrap()
            }
            _ => panic!("wrong command"),
        };
        assert_eq!(prefix.0, [0, 255]);
        assert!(
            parse_console_line(&format!("{command} BinaryObject --payload-prefix 0x0")).is_err()
        );
        assert!(
            parse_console_line(&format!("{command} BinaryObject --payload-prefix {max}00"))
                .is_err()
        );
    }
}

#[test]
fn console_quotes_preserve_exact_fluent_operands() {
    for (operand, expected) in [
        ("''", ""),
        (r#""a b""#, "a b"),
        (r"a\ b", "a b"),
        (r#""say \"hi\"""#, "say \"hi\""),
        ("'é 🦀'", "é 🦀"),
        (
            "'$HOME $(whoami) `whoami` *.rs ; |'",
            "$HOME $(whoami) `whoami` *.rs ; |",
        ),
    ] {
        for command in ["query", "aggregate"] {
            let cli = parse_console_line(&format!(
                "{command} Document --select min:note --group-min {operand}\r\n"
            ))
            .unwrap();

            match cli.command.unwrap() {
                Command::Query { group_bounds, .. } | Command::Aggregate { group_bounds, .. } => {
                    assert_eq!(group_bounds.group_min.as_deref(), Some(expected));
                }
                _ => panic!("wrong fluent command"),
            }
        }
    }
    assert!(
        matches!(parse_console_line("count 'Document'").unwrap().command,
        Some(Command::Count { entity }) if entity == "Document")
    );
}

#[test]
fn incomplete_console_quotes_and_escapes_refuse_before_admission() {
    for line in [
        "count 'Document",
        "count \"Document",
        "count Document\\",
        "count Document\\\n",
    ] {
        let error = parse_console_line(line)
            .err()
            .expect("incomplete command must refuse");
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
        assert!(
            error
                .to_string()
                .contains("unterminated quote or trailing escape")
        );
    }
}

#[test]
fn table_controls_cannot_become_foreground_input_requests() {
    let table = ic_host_text::bordered_table(&["Field"], &[vec!["row\u{5}\n\u{202e}"]]);
    assert!(table.contains(r"row\u{5}\n\u{202e}"));
    assert!(!table.as_bytes().contains(&5));
    assert_eq!(table.lines().count(), 5);
}

#[test]
fn cycle_display_preserves_exact_charges_and_does_not_hide_balance_increases() {
    assert_eq!(charged_cycles(100, 100), "0 cycles charged");
    assert_eq!(charged_cycles(110, 100), "10 cycles charged");
    assert_eq!(
        charged_cycles(5_449_999, 0),
        "5.4M cycles charged (5449999)"
    );
    assert_eq!(
        charged_cycles(5_450_000, 0),
        "5.5M cycles charged (5450000)"
    );
    assert_eq!(
        charged_cycles(9_999_999, 0),
        "10.0M cycles charged (9999999)"
    );
    assert_eq!(
        charged_cycles(u128::MAX, u128::MAX - 5_400_001),
        "5.4M cycles charged (5400001)"
    );
    assert_eq!(
        charged_cycles(100, 110),
        "cycle charge unavailable (balance increased by 10)"
    );
}

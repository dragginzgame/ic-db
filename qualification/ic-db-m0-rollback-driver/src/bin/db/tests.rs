use super::{Command, charged_cycles, parse_console_line};

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
                Command::Query { group_min, .. } | Command::Aggregate { group_min, .. } => {
                    assert_eq!(group_min.as_deref(), Some(expected));
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

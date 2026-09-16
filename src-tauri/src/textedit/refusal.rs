//! Explain rejected operations without copying document-controlled values into errors.
use lopdf::Object;

pub(super) fn operation(name: &str, operands: &[Object], inside: bool, positioned: bool) -> String {
    let reason = match name {
        "BMC" | "BDC" | "EMC" if inside => {
            return format!("inline {name} marked content is not editable yet");
        }
        "BT" if inside => "nested text blocks are not editable",
        "ET" if !inside => "text block ends without a matching beginning",
        "Tj" | "TJ" if !inside => "text appears outside a text block",
        "Tj" | "TJ" if !positioned => "text has no explicit initial position",
        "Tj" | "TJ" => "text-show operands have an unsupported shape",
        "'" | "\"" => {
            return format!("text positioning shorthand {name} is not editable yet");
        }
        "Ts" if operands.len() == 1 => "text rise must be zero for editing",
        "Tz" if operands.len() == 1 => "text horizontal scaling must be 100 percent for editing",
        "Tr" if matches!(operands, [Object::Integer(_)]) => "only filled text is editable",
        "q" | "Q" | "cm" | "re" | "m" | "n" | "Do" if inside => {
            return format!("graphics operation {name} inside a text block is not editable yet");
        }
        "Tf" | "TL" | "Tm" | "Td" | "TD" | "T*" if !inside => {
            return format!("text setup operation {name} outside a text block is not editable yet");
        }
        // Only fixed PDF keywords may be echoed. An unknown keyword could be
        // arbitrarily long, contain control characters or carry document text.
        "BMC" | "BDC" | "EMC" | "BT" | "ET" | "q" | "Q" | "cm" | "n" | "Do" | "w" | "Tc" | "Tw"
        | "Ts" | "Tz" | "Tr" | "ri" | "gs" | "cs" | "CS" | "Tf" | "TL" | "Tm" | "Td" | "TD"
        | "T*" => {
            return format!(
                "unsupported operands for {name} (count: {})",
                operands.len()
            );
        }
        "l" | "c" | "v" | "y" | "h" | "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*"
        | "W" | "W*" => {
            return format!("path operation {name} is outside a supported complete path");
        }
        "sh" => "shading patterns are not editable yet",
        "BI" | "ID" | "EI" => "inline images are not editable yet",
        "MP" | "DP" | "BX" | "EX" | "d0" | "d1" => {
            return format!("content operation {name} is not editable yet");
        }
        _ => "unrecognized content operator is not editable yet",
    };
    reason.into()
}

#[cfg(test)]
mod tests {
    use crate::textedit::{scan, tests::with_content};

    #[test]
    fn textedit_refusals_distinguish_operator_context_without_echoing_values() {
        for (source, expected) in [
            (
                "BT /Span << /ActualText (SYNTHETIC SECRET) >> BDC ET",
                "inline BDC marked content is not editable yet",
            ),
            (
                "BT /Span BMC ET",
                "inline BMC marked content is not editable yet",
            ),
            ("BT EMC ET", "inline EMC marked content is not editable yet"),
            ("BT BT ET", "nested text blocks are not editable"),
            ("ET", "text block ends without a matching beginning"),
            ("(SYNTHETIC SECRET) Tj", "text appears outside a text block"),
            (
                "BT /F1 12 Tf (SYNTHETIC SECRET) Tj ET",
                "text has no explicit initial position",
            ),
            (
                "BT /F1 12 Tf 40 180 Td (SYNTHETIC SECRET) TJ ET",
                "text-show operands have an unsupported shape",
            ),
            (
                "0 -12 TD",
                "text setup operation TD outside a text block is not editable yet",
            ),
            ("BT 0 TD ET", "unsupported operands for TD (count: 1)"),
            (
                "BT (SYNTHETIC SECRET) ' ET",
                "text positioning shorthand ' is not editable yet",
            ),
            (
                "BT 0 0 (SYNTHETIC SECRET) \" ET",
                "text positioning shorthand \" is not editable yet",
            ),
            ("Ts", "unsupported operands for Ts (count: 0)"),
            ("Tz", "unsupported operands for Tz (count: 0)"),
            ("Tr", "unsupported operands for Tr (count: 0)"),
            ("2 Ts", "text rise must be zero for editing"),
            (
                "90 Tz",
                "text horizontal scaling must be 100 percent for editing",
            ),
            ("3 Tr", "only filled text is editable"),
            ("0.0 Tr", "unsupported operands for Tr (count: 1)"),
            (
                "BT q ET",
                "graphics operation q inside a text block is not editable yet",
            ),
            (
                "12 TL",
                "text setup operation TL outside a text block is not editable yet",
            ),
            ("BT /F1 Tf ET", "unsupported operands for Tf (count: 1)"),
            (
                "0 0 l",
                "path operation l is outside a supported complete path",
            ),
            (
                "/SYNTHETIC_SECRET sh",
                "shading patterns are not editable yet",
            ),
            (
                "/SYNTHETIC_SECRET MP",
                "content operation MP is not editable yet",
            ),
        ] {
            let error = scan(&with_content(source.as_bytes()), 0).unwrap_err();
            assert_eq!(error, expected, "{source}");
            assert!(!error.contains("SECRET"));
        }
        let huge = "SYNTHETIC_SECRET".repeat(1000);
        assert_eq!(
            super::operation(&huge, &[], false, false),
            "unrecognized content operator is not editable yet"
        );
        assert!(scan(
            &with_content(b"BT /F1 12 Tf 40 180 Td (SYNTHETIC) Tj ET"),
            0
        )
        .is_ok());
    }
}

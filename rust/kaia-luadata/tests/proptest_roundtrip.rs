//! Property tests: any value the writer produces parses back to the same value.

mod common;

use common::{arb_value, same};
use kaia_luadata::{parse, to_string};
use proptest::prelude::*;

proptest! {
    #[test]
    fn to_string_then_parse_gives_the_same_value(v in arb_value()) {
        let text = to_string(&v);
        let back = parse(&text).map_err(|e| TestCaseError::fail(format!("{e}\n{text}")))?;
        prop_assert!(same(&v, &back), "changed in a round trip:\n{text}");
    }

    #[test]
    fn writing_twice_gives_the_same_text(v in arb_value()) {
        let text = to_string(&v);
        let back = parse(&text).unwrap();
        prop_assert_eq!(to_string(&back), text);
    }

    #[test]
    fn finite_numbers_print_as_plain_numerals(n in any::<f64>().prop_filter("finite", |n| n.is_finite())) {
        let v = kaia_luadata::Value::Number(n);
        let text = to_string(&v);
        // The numeral never uses the division spelling for finite values.
        prop_assert!(!text.contains('/'));
        prop_assert!(same(&v, &parse(&text).unwrap()));
    }
}

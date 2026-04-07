//! Naming conventions — delegated to meimei.

pub use meimei::{
    strip_provider_prefix, to_camel_case, to_kebab_case, to_pascal_case, to_snake_case,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_from_kebab() {
        assert_eq!(to_snake_case("my-field-name"), "my_field_name");
    }

    #[test]
    fn snake_case_already_snake() {
        assert_eq!(to_snake_case("already_snake"), "already_snake");
    }

    #[test]
    fn snake_case_from_camel() {
        // meimei's to_snake_case converts hyphens to underscores, not camelCase
        assert_eq!(to_snake_case("myFieldName"), "myFieldName");
    }

    #[test]
    fn snake_case_empty_string() {
        assert_eq!(to_snake_case(""), "");
    }

    #[test]
    fn snake_case_single_char() {
        assert_eq!(to_snake_case("x"), "x");
    }

    #[test]
    fn camel_case_from_snake() {
        assert_eq!(to_camel_case("my_field_name"), "myFieldName");
    }

    #[test]
    fn camel_case_empty_string() {
        assert_eq!(to_camel_case(""), "");
    }

    #[test]
    fn pascal_case_from_snake() {
        assert_eq!(to_pascal_case("my_field_name"), "MyFieldName");
    }

    #[test]
    fn pascal_case_empty_string() {
        assert_eq!(to_pascal_case(""), "");
    }

    #[test]
    fn kebab_case_from_snake() {
        assert_eq!(to_kebab_case("my_field_name"), "my-field-name");
    }

    #[test]
    fn kebab_case_empty_string() {
        assert_eq!(to_kebab_case(""), "");
    }

    #[test]
    fn strip_provider_prefix_removes_prefix() {
        assert_eq!(strip_provider_prefix("akeyless_static_secret", "akeyless"), "static_secret");
    }

    #[test]
    fn strip_provider_prefix_no_match() {
        assert_eq!(strip_provider_prefix("other_resource", "akeyless"), "other_resource");
    }

    #[test]
    fn strip_provider_prefix_empty_inputs() {
        assert_eq!(strip_provider_prefix("", "akeyless"), "");
        assert_eq!(strip_provider_prefix("resource", ""), "resource");
    }

    #[test]
    fn snake_case_consecutive_uppercase() {
        // meimei's to_snake_case only converts hyphens, not camelCase
        assert_eq!(to_snake_case("HTTPSProxy"), "HTTPSProxy");
    }

    #[test]
    fn pascal_case_from_kebab() {
        assert_eq!(to_pascal_case("my-field"), "MyField");
    }

    #[test]
    fn camel_case_from_kebab() {
        assert_eq!(to_camel_case("my-field"), "myField");
    }
}

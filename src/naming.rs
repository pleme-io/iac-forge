/// Convert a hyphenated or snake_case name to `PascalCase`.
///
/// Examples: `bound-aws-account-id` -> `BoundAwsAccountId`,
///           `access_expires` -> `AccessExpires`
#[must_use]
pub fn to_pascal_case(name: &str) -> String {
    name.split(|c: char| c == '-' || c == '_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{upper}{}", chars.as_str())
                }
                None => String::new(),
            }
        })
        .collect()
}

/// Convert a name to `snake_case` (hyphens become underscores).
#[must_use]
pub fn to_snake_case(name: &str) -> String {
    name.replace('-', "_")
}

/// Convert a name to `camelCase`.
///
/// Example: `bound-aws-account-id` -> `boundAwsAccountId`
#[must_use]
pub fn to_camel_case(name: &str) -> String {
    let pascal = to_pascal_case(name);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(c) => {
            let lower: String = c.to_lowercase().collect();
            format!("{lower}{}", chars.as_str())
        }
        None => String::new(),
    }
}

/// Convert a name to `kebab-case` (underscores become hyphens).
#[must_use]
pub fn to_kebab_case(name: &str) -> String {
    name.replace('_', "-")
}

/// Strip a common provider prefix from a resource name.
///
/// Example: `akeyless_static_secret` with prefix `akeyless` -> `static_secret`
#[must_use]
pub fn strip_provider_prefix<'a>(name: &'a str, provider: &str) -> &'a str {
    let prefix = format!("{provider}_");
    name.strip_prefix(&prefix).unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case() {
        assert_eq!(to_pascal_case("bound-aws-account-id"), "BoundAwsAccountId");
        assert_eq!(to_pascal_case("access_expires"), "AccessExpires");
        assert_eq!(to_pascal_case("name"), "Name");
        assert_eq!(to_pascal_case("static_secret"), "StaticSecret");
        assert_eq!(to_pascal_case("a-b_c"), "ABC");
    }

    #[test]
    fn snake_case() {
        assert_eq!(to_snake_case("bound-aws-account-id"), "bound_aws_account_id");
        assert_eq!(to_snake_case("delete_protection"), "delete_protection");
    }

    #[test]
    fn camel_case() {
        assert_eq!(to_camel_case("bound-aws-account-id"), "boundAwsAccountId");
        assert_eq!(to_camel_case("access_expires"), "accessExpires");
        assert_eq!(to_camel_case("name"), "name");
    }

    #[test]
    fn kebab_case() {
        assert_eq!(to_kebab_case("bound_aws_account_id"), "bound-aws-account-id");
        assert_eq!(to_kebab_case("static-secret"), "static-secret");
    }

    #[test]
    fn strip_prefix() {
        assert_eq!(
            strip_provider_prefix("akeyless_static_secret", "akeyless"),
            "static_secret"
        );
        assert_eq!(
            strip_provider_prefix("some_resource", "akeyless"),
            "some_resource"
        );
    }
}

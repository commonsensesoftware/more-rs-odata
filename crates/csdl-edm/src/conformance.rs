//! Authoring checks are opt-in; tolerant consumers retain forward compatibility.
use unicode_properties::{GeneralCategory as G, UnicodeGeneralCategory};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ValidationProfile {
    #[default]
    Tolerant,
    Strict,
}

/// The document's intended use, independent of strictness.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DocumentKind {
    #[default]
    Model,
    ServiceMetadata,
}

/// CSDL 4.01 section 15.2, measured in Unicode code points.
pub fn is_simple_identifier(value: &str) -> bool {
    fn first(c: char) -> bool {
        c == '_'
            || matches!(
                c.general_category(),
                G::UppercaseLetter
                    | G::LowercaseLetter
                    | G::TitlecaseLetter
                    | G::ModifierLetter
                    | G::OtherLetter
                    | G::LetterNumber
            )
    }
    let mut chars = value.chars();
    (1..=128).contains(&value.chars().count())
        && chars.next().is_some_and(first)
        && chars.all(|c| {
            first(c)
                || matches!(
                    c.general_category(),
                    G::DecimalNumber
                        | G::NonspacingMark
                        | G::SpacingMark
                        | G::ConnectorPunctuation
                        | G::Format
                )
        })
}

pub fn is_namespace(value: &str) -> bool {
    value.chars().count() <= 511 && value.split('.').all(is_simple_identifier)
}

pub fn is_qualified_name(value: &str) -> bool {
    value
        .rsplit_once('.')
        .is_some_and(|(ns, name)| is_namespace(ns) && is_simple_identifier(name))
}

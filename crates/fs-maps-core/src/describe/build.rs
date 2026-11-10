//! Small builders shared by the descriptions: label keys, the reference to a record, the search command.
//!
//! Pattern: Builder helpers — functions over `FieldDescription` / `CommandDescription` of `fs-types`.

use fs_types::{BasicType, CommandDescription, CommandKind, FieldDescription};

/// `elevationProfile` -> `elevation-profile`: label keys are lower case (`fs-types` rule).
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// `<type>.<name>`: the label key of a field.
pub(crate) fn field(ty: &str, name: &str, basic: BasicType) -> FieldDescription {
    FieldDescription::new(name, basic).label(&format!("{ty}.{}", kebab(name)))
}

/// The reference to a record of this type, the field that tells a frontend the command works on one record.
pub(crate) fn record(ty: &str, target: &str) -> FieldDescription {
    field(ty, "id", BasicType::Reference)
        .required()
        .target(target)
}

/// `<type>.cmd.<name>`: a command with its label key.
pub(crate) fn command(ty: &str, name: &str, kind: CommandKind) -> CommandDescription {
    CommandDescription::new(name, kind, &format!("{ty}.cmd.{}", kebab(name)))
}

/// The search of the list: a word that the name contains.
pub(crate) fn find(ty: &str) -> CommandDescription {
    command(ty, "find", CommandKind::Find).input(field(ty, "term", BasicType::Text))
}

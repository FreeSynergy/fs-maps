//! Which commands the core carries out: a table, until the type description reports it itself.
//!
//! Pattern: Specification — one table decides per command of each type; nothing else in the program knows. Provisional
//! (reported): `types.capnp` has no field `supported` in `CommandDescription` (Thread T adds it); when it does, the
//! description reports this table and the frontend unlocks by itself. Switch a command on **here only**.

/// One row: the data type, the command, whether the core carries it out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Support {
    /// The type name (`Place`).
    pub data_type: &'static str,
    /// The command name (`create`).
    pub command: &'static str,
    /// Whether the core carries it out.
    pub supported: bool,
}

const fn row(data_type: &'static str, command: &'static str, supported: bool) -> Support {
    Support {
        data_type,
        command,
        supported,
    }
}

/// Every command of every type. A test keeps this table and the descriptions in step, and `DESIGN.md` listing
/// `Unsupported` in step with it.
pub const SUPPORT: &[Support] = &[
    row("Place", "find", true),
    row("Place", "within", true),
    row("Place", "create", true),
    row("Place", "change", true),
    row("Place", "delete", true),
    row("Place", "elevationProfile", false),
    row("MapLayer", "find", false),
    row("MapLayer", "create", false),
    row("MapLayer", "change", false),
    row("MapLayer", "delete", false),
    row("MapLayer", "refresh", false),
    row("MapLayer", "invite", false),
    row("MapLayer", "comment", false),
    row("MapLayer", "webhook", false),
    row("MapTour", "find", false),
    row("MapTour", "create", false),
    row("MapTour", "change", false),
    row("MapTour", "delete", false),
    row("Region", "find", false),
    row("Region", "estimate", false),
    row("Region", "download", false),
    row("Region", "delete", false),
];

impl Support {
    /// Whether the core carries out `command` of `data_type`; an unknown command is not.
    #[must_use]
    pub fn is(data_type: &str, command: &str) -> bool {
        SUPPORT
            .iter()
            .any(|r| r.data_type == data_type && r.command == command && r.supported)
    }
}

/// The rows that are not supported.
pub fn unsupported() -> impl Iterator<Item = &'static Support> {
    SUPPORT.iter().filter(|r| !r.supported)
}

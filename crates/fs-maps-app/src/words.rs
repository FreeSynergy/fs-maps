//! The words of the program (ADR 0057): all English text of this frontend in one place.
//!
//! Pattern: Strategy (`Say` of the components) over a table — a key in, English out. Translation (ADR 0053) will
//! replace this file's content, not its users. A key the table does not know is shown as its last part, spelled out.

use fs_components_fields::Say;

/// The title of the window.
pub const TITLE: &str = "FreeSynergy Maps";

const TABLE: &[(&str, &str)] = &[
    ("action.add", "Add"),
    ("action.back", "Back"),
    ("action.cancel", "Cancel"),
    ("action.close", "Close"),
    ("action.remove", "Remove"),
    ("action.save", "Save"),
    ("menu.program", "Program"),
    ("menu.quit", "Quit"),
    ("menu.view", "View"),
    ("menu.help", "Help"),
    ("menu.about", "About"),
    ("menu.map", "Map"),
    ("view.map", "Map"),
    ("message.done", "Done."),
    ("message.rejected", "The program did not accept this."),
    ("message.not-found", "There is no such record."),
    (
        "message.unavailable",
        "The maps program cannot be reached or cannot do this yet.",
    ),
    (
        "message.unsaved",
        "There is unsaved text; save or cancel it first.",
    ),
    ("list.empty", "Nothing here yet."),
    ("list.stale", "Not up to date"),
    ("detail.not-loaded", "This record is not loaded."),
    ("confirm.sure", "Are you sure?"),
    ("value.none", "-"),
    ("about.version", "Version"),
    ("about.origin", "Source"),
    ("about.licence", "Licence"),
    ("about.repository", "Repository"),
    ("place", "Places"),
    ("layer", "Map layers"),
    ("tour", "Tours"),
    ("region", "Offline regions"),
    ("place.link", "Link"),
    ("place.cmd.within", "Within a box"),
    ("place.southwest", "South-west corner"),
    ("place.northeast", "North-east corner"),
    ("place.position", "Position (geo: URI)"),
    ("region.min-zoom", "Smallest zoom"),
    ("region.max-zoom", "Largest zoom"),
];

/// English for the keys of this program and the frontend.
#[derive(Clone, Copy, Debug, Default)]
pub struct English;

fn spelled(key: &str) -> String {
    let last = key.rsplit('.').next().unwrap_or(key).replace('-', " ");
    let mut chars = last.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

impl Say for English {
    fn say(&self, key: &str) -> String {
        TABLE
            .iter()
            .find(|(k, _)| *k == key)
            .map_or_else(|| spelled(key), |(_, v)| (*v).to_owned())
    }
}

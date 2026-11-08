//! The words of the program (ADR 0057): all English text of this frontend in one place.
//!
//! Pattern: Value objects — plain texts; translation (ADR 0053) will replace this file's content,
//! not its users.

use fs_components_workbenches::people::PeopleLabels;

/// The title of the window.
pub const TITLE: &str = "FreeSynergy Maps";

/// The words of the workbench (it is the list-and-page workbench of people, used for places).
pub fn labels() -> PeopleLabels {
    let mut labels = PeopleLabels::default();
    labels.search = "Search places".into();
    labels.columns = vec!["Name".into(), "Position".into()];
    labels
}

/// Headings and labels of what is shown about a place.
pub(crate) mod field {
    pub const NAME: &str = "Name";
    pub const LATITUDE: &str = "Latitude";
    pub const LONGITUDE: &str = "Longitude";
    pub const DESCRIPTION: &str = "Description";
    pub const LOCATION: &str = "Location";
    pub const ABOUT: &str = "About";
    pub const GEO: &str = "Geo URI";
    pub const SHAPE: &str = "Shape";
    pub const NO_NAME: &str = "A place needs a name.";
    pub const BAD_LATITUDE: &str = "The latitude is a number from -90 to 90, like 52.52.";
    pub const BAD_LONGITUDE: &str = "The longitude is a number from -180 to 180, like 13.405.";
    pub const NOT_A_POINT: &str = "This place is not a single point; its shape is kept as it is.";
}

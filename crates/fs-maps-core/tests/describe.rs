//! The type descriptions of fs-maps: valid, complete against the requirements, in step with the support table and
//! with `DESIGN.md`, and unchanged across the wire.
//!
//! Pattern: none (tests against the trait `DescribesType` and the registry).

use fs_interfaces::types_capnp::type_catalog;
use fs_maps_core::describe::{
    SUPPORT, Support, read_catalog, registry, unsupported, write_catalog,
};
use fs_types::{BasicType, CommandKind};

const DESIGN: &str = include_str!("../../../DESIGN.md");

#[test]
fn all_four_types_are_valid_and_registered() {
    let r = registry();
    let mut names: Vec<&str> = r.iter().map(|t| t.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["MapLayer", "MapTour", "Place", "Region"]);
    assert_eq!(r.len(), 4);
    assert!(r.unresolved_targets().iter().all(|(_, _, t)| t == "Person"), "only the people program is outside");
}

#[test]
fn place_follows_geojson_and_geo_uri() {
    let r = registry();
    let p = r.get("Place").unwrap();
    assert!(p.standard.contains("RFC 7946") && p.standard.contains("RFC 5870"));
    assert_eq!(p.find_field("position").unwrap().basic, BasicType::Place);
    assert!(p.find_field("name").unwrap().required);
}

#[test]
fn every_command_of_the_core_and_of_the_requirements_is_described() {
    let r = registry();
    // The core: create, update (change), delete, get/list (find), within, find_by_name (find), region download.
    for (ty, cmd) in [
        ("Place", "create"),
        ("Place", "change"),
        ("Place", "delete"),
        ("Place", "find"),
        ("Place", "within"),
        ("Region", "estimate"),
        ("Region", "download"),
        // requirements.md, optional list: live editing, tours, elevation profile, live layers, comments, webhooks.
        ("MapLayer", "invite"),
        ("MapTour", "create"),
        ("Place", "elevationProfile"),
        ("MapLayer", "refresh"),
        ("MapLayer", "comment"),
        ("MapLayer", "webhook"),
    ] {
        assert!(r.get(ty).and_then(|t| t.find_command(cmd)).is_some(), "{ty}.{cmd}");
    }
}

#[test]
fn the_support_table_and_the_descriptions_name_the_same_commands() {
    let r = registry();
    let mut described: Vec<(String, String)> = r
        .iter()
        .flat_map(|t| t.commands.iter().map(|c| (t.name.clone(), c.name.clone())))
        .collect();
    let mut table: Vec<(String, String)> = SUPPORT
        .iter()
        .map(|s| (s.data_type.to_owned(), s.command.to_owned()))
        .collect();
    described.sort();
    table.sort();
    assert_eq!(described, table);
    assert!(Support::is("Place", "create"));
    assert!(!Support::is("Place", "elevationProfile"));
    assert!(!Support::is("Place", "nothing"));
}

#[test]
fn design_lists_every_unsupported_command() {
    let section = DESIGN.split("## Unsupported").nth(1).expect("DESIGN.md has the section `Unsupported`");
    for s in unsupported() {
        let key = format!("`{}.{}`", s.data_type, s.command);
        assert!(section.contains(&key), "{key} is missing under `Unsupported` in DESIGN.md");
    }
}

#[test]
fn a_command_on_one_record_names_its_own_type() {
    for t in registry().iter() {
        for c in &t.commands {
            if matches!(c.kind, CommandKind::Change | CommandKind::Delete) {
                assert!(
                    c.input.iter().any(|f| f.basic == BasicType::Reference && f.target == t.name),
                    "{}.{} must refer to the record it works on",
                    t.name,
                    c.name
                );
            }
        }
    }
}

#[test]
fn descriptions_survive_the_wire() {
    let r = registry();
    let all: Vec<_> = r.iter().cloned().collect();
    let mut message = capnp::message::Builder::new_default();
    {
        let root = message.init_root::<type_catalog::describe_results::Builder<'_>>();
        write_catalog(&all, root.init_types(u32::try_from(all.len()).unwrap()));
    }
    let reader = message
        .get_root_as_reader::<type_catalog::describe_results::Reader<'_>>()
        .unwrap();
    assert_eq!(read_catalog(reader.get_types().unwrap()).unwrap(), all);
}

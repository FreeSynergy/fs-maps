//! The type descriptions on the wire (`TypeCatalog`, `types.capnp` of `fs-interfaces`).
//!
//! Pattern: Adapter — the descriptions are `fs-types` values (the version the admin frontend uses); the contract here
//! is the newer `fs-interfaces`, a different crate with the same message layout. The enums cross as numbers
//! (the schema promise: only append), a kind this side does not know reads as `unknown`.

use fs_interfaces::types_capnp as wire;
use fs_types::{
    BasicType, CommandDescription, CommandKind, FieldDescription, TypeDescription,
};

fn len(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn write_fields(fields: &[FieldDescription], mut list: capnp::struct_list::Builder<'_, wire::field_description::Owned>) {
    for (i, f) in (0..).zip(fields) {
        let mut b = list.reborrow().get(i);
        b.set_name(&f.name);
        b.set_basic(wire::BasicType::try_from(f.basic as u16).unwrap_or(wire::BasicType::Unknown));
        b.set_required(f.required);
        b.set_multiple(f.multiple);
        b.set_label(&f.label);
        b.set_target(&f.target);
        let mut c = b.init_choices(len(f.choices.len()));
        for (j, k) in (0..).zip(&f.choices) {
            c.set(j, k.as_str());
        }
    }
}

/// Write the descriptions (the answer of `Places.types` → `TypeCatalog.describe`).
pub fn write_catalog(
    types: &[TypeDescription],
    mut list: capnp::struct_list::Builder<'_, wire::type_description::Owned>,
) {
    for (i, t) in (0..).zip(types) {
        let mut b = list.reborrow().get(i);
        b.set_name(&t.name);
        b.set_program(&t.program);
        b.set_standard(&t.standard);
        b.set_label(&t.label);
        write_fields(&t.fields, b.reborrow().init_fields(len(t.fields.len())));
        let mut cmds = b.init_commands(len(t.commands.len()));
        for (j, c) in (0..).zip(&t.commands) {
            let mut cb = cmds.reborrow().get(j);
            cb.set_name(&c.name);
            cb.set_kind(wire::CommandKind::try_from(c.kind as u16).unwrap_or(wire::CommandKind::Unknown));
            cb.set_label(&c.label);
            write_fields(&c.input, cb.reborrow().init_input(len(c.input.len())));
            write_fields(&c.result, cb.init_result(len(c.result.len())));
        }
    }
}

fn read_fields(list: capnp::struct_list::Reader<'_, wire::field_description::Owned>) -> capnp::Result<Vec<FieldDescription>> {
    list.iter()
        .map(|r| {
            let choices = r
                .get_choices()?
                .iter()
                .map(|c| Ok(c?.to_string()?))
                .collect::<capnp::Result<Vec<_>>>()?;
            Ok(FieldDescription {
                name: r.get_name()?.to_string()?,
                basic: BasicType::try_from(r.get_basic().map_or(0, |b| b as u16)).unwrap_or(BasicType::Unknown),
                required: r.get_required(),
                multiple: r.get_multiple(),
                label: r.get_label()?.to_string()?,
                target: r.get_target()?.to_string()?,
                choices,
            })
        })
        .collect()
}

/// Read the descriptions of a catalog answer.
///
/// # Errors
/// A damaged message or a text that is not UTF-8.
pub fn read_catalog(
    list: capnp::struct_list::Reader<'_, wire::type_description::Owned>,
) -> capnp::Result<Vec<TypeDescription>> {
    list.iter()
        .map(|t| {
            let commands = t
                .get_commands()?
                .iter()
                .map(|c| {
                    Ok(CommandDescription {
                        name: c.get_name()?.to_string()?,
                        kind: CommandKind::try_from(c.get_kind().map_or(0, |k| k as u16)).unwrap_or(CommandKind::Unknown),
                        label: c.get_label()?.to_string()?,
                        input: read_fields(c.get_input()?)?,
                        result: read_fields(c.get_result()?)?,
                    })
                })
                .collect::<capnp::Result<Vec<_>>>()?;
            Ok(TypeDescription {
                name: t.get_name()?.to_string()?,
                program: t.get_program()?.to_string()?,
                standard: t.get_standard()?.to_string()?,
                label: t.get_label()?.to_string()?,
                fields: read_fields(t.get_fields()?)?,
                commands,
            })
        })
        .collect()
}

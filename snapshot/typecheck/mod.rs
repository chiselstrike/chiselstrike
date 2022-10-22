use anyhow::{Result, bail, ensure};
use std::collections::HashSet;
use std::sync::Arc;
use crate::schema;

mod recursive;
use self::recursive::{Relation, evaluate_relation};

/// Variant of the typechecking algorithm.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub enum TypeVariant {
    /// Typechecking on the level of TypeScript entities.
    ///
    /// This is a bit stricter than TypeScript itself. For example, `Id<E>` is treated as a
    /// different type than `E["id"]`.
    Entity,

    /// Typechecking on the level of plain objects that are returned by the `datastore` crate.
    ///
    /// The notable difference is that eager references (`E`) are represented and treated as ids
    /// (`Id<E>`).
    Plain,
}

/// Decides wheter `src_type` is a subtype of `tgt_type`.
///
/// A type _S_ is a subtype of type _T_ (written _S <: T_) if _S_ can be used whenever _T_ is
/// expected.
pub fn is_subtype(
    src_schema: &schema::Schema,
    src_type: &Arc<schema::Type>,
    tgt_schema: &schema::Schema,
    tgt_type: &Arc<schema::Type>,
    variant: TypeVariant,
) -> Result<()> {
    evaluate_relation(src_schema, tgt_schema, (src_type.clone(), tgt_type.clone()),
        |goal, goals| step(src_schema, tgt_schema, goal, variant, goals))
}

fn step(
    src_schema: &schema::Schema,
    tgt_schema: &schema::Schema,
    goal: Relation,
    variant: TypeVariant,
    goals: &mut Vec<Relation>,
) -> Result<()> {
    let (src_type, tgt_type) = goal;

    if let Some(tgt_type) = is_optional(tgt_schema, &tgt_type) {
        let src_type = is_optional(src_schema, &src_type).unwrap_or(&src_type);
        goals.push((src_type.clone(), tgt_type.clone()));
        return Ok(());
    }

    match (&*src_type, &*tgt_type) {
        (schema::Type::Ref(src_name, src_kind), schema::Type::Ref(tgt_name, tgt_kind)) => {
            ensure!(src_name == tgt_name,
                "references to entities {:?} and {:?} are not compatible",
                src_name, tgt_name);
            if variant == TypeVariant::Entity {
                ensure!(src_kind == tgt_kind,
                    "references {:?} and {:?} are not compatible", src_kind, tgt_kind);
            }
        },
        (schema::Type::Primitive(src_type), schema::Type::Primitive(tgt_type)) =>
            ensure!(is_primitive_subtype(*src_type, *tgt_type),
                "primitive type {:?} cannot be used in place of {:?}",
                src_type, tgt_type),
        (schema::Type::Array(src_type), schema::Type::Array(tgt_type)) =>
            goals.push((src_type.clone(), tgt_type.clone())),
        (schema::Type::Object(src_obj), schema::Type::Object(tgt_obj)) => {
            for tgt_field in tgt_obj.fields.values() {
                if let Some(src_field) = src_obj.fields.get(&tgt_field.name) {
                    ensure!(!src_field.optional || tgt_field.optional,
                        "object field {:?} is required, but it was marked as optional", tgt_field.name);
                    goals.push((src_field.type_.clone(), tgt_field.type_.clone()));
                } else {
                    ensure!(tgt_field.optional,
                        "object field {:?} is required, but it is not present", tgt_field.name);
                }
            }
        },
        (_, _) =>
            bail!("types {:?} and {:?} are not compatible", src_type, tgt_type),
    }
    Ok(())
}

pub fn is_primitive_subtype(src_type: schema::PrimitiveType, tgt_type: schema::PrimitiveType) -> bool {
    src_type == tgt_type || matches!((src_type, tgt_type),
        (schema::PrimitiveType::Uuid, schema::PrimitiveType::String)
    )
}

/// Decides whether `type_` is of the form `T | undefined`, and if it is, returns `T`.
///
/// Notably, this handles typedefs and nested optionals (it returns `T` for type `(T |
/// undefined) | undefined`.
pub fn is_optional<'s>(
    schema: &'s schema::Schema,
    type_: &'s Arc<schema::Type>,
) -> Option<&'s Arc<schema::Type>> {
    let mut result = None;
    let mut assumptions = HashSet::new();
    let mut goal = type_;
    while assumptions.insert(goal) {
        match &**goal {
            schema::Type::Typedef(type_name) =>
                goal = &schema.typedefs[type_name],
            schema::Type::Optional(opt_type) => {
                result = Some(opt_type);
                goal = opt_type;
            },
            _ => break,
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! schema {
        ($($json:tt)+) => {
            serde_json::from_value::<schema::Schema>(serde_json::json!($($json)+))
                .expect("could not deserialize schema")
        }
    }

    macro_rules! type_ {
        ($($json:tt)+) => {
            serde_json::from_value::<Arc<schema::Type>>(serde_json::json!($($json)+))
                .expect("could not deserialize type")
        }
    }

    macro_rules! type_name {
        ($name:expr) => {
            type_name!("", $name)
        };
        ($module:expr, $name:expr) => {
            schema::TypeName {
                module: $module.into(),
                name: $name.into(),
            }
        };
    }

    macro_rules! _is_subtype {
        ($src_ty:tt, $tgt_ty:tt) => {
            _is_subtype!($src_ty, $tgt_ty, variant: Plain)
        };
        ($src_ty:tt, $tgt_ty:tt, variant: $variant:ident) => {{
            let empty_schema = schema::Schema::default();
            _is_subtype!(empty_schema, $src_ty, $tgt_ty, variant: $variant)
        }};
        ($schema:expr, $src_ty:tt, $tgt_ty:tt) => {
            _is_subtype!($schema, $src_ty, $tgt_ty, variant: Plain)
        };
        ($schema:expr, $src_ty:tt, $tgt_ty:tt, variant: $variant:ident) => {{
            let schema = &$schema;
            _is_subtype!(schema, $src_ty, schema, $tgt_ty, variant: $variant)
        }};
        ($src_schema:expr, $src_ty:tt, $tgt_schema:expr, $tgt_ty:tt, variant: $variant:ident) => {{
            let src_schema = &$src_schema;
            let tgt_schema = &$tgt_schema;
            let src_ty = type_!($src_ty);
            let tgt_ty = type_!($tgt_ty);
            let res = is_subtype(src_schema, &src_ty, tgt_schema, &tgt_ty, TypeVariant::$variant);
            (src_ty, tgt_ty, res)
        }};
    }

    macro_rules! assert_subtype {
        ($($args:tt)*) => {
            let (src_ty, tgt_ty, res) = _is_subtype!($($args)*);
            if let Err(err) = res {
                panic!("{:?} should be a subtype of {:?}: {:#}", src_ty, tgt_ty, err);
            }
        }
    }

    macro_rules! assert_not_subtype {
        ($($args:tt)*) => {
            let (src_ty, tgt_ty, res) = _is_subtype!($($args)*);
            if res.is_ok() {
                panic!("{:?} should not be a subtype of {:?}", src_ty, tgt_ty);
            }
        }
    }

    #[test]
    fn test_subtype_primitives() {
        // the relation must be reflexive
        assert_subtype!({"primitive": "string"}, {"primitive": "string"});
        assert_subtype!({"primitive": "number"}, {"primitive": "number"});

        // incompatible primitives
        assert_not_subtype!({"primitive": "number"}, {"primitive": "string"});
        assert_not_subtype!({"primitive": "boolean"}, {"primitive": "jsDate"});

        // uuid is a subtype of string
        assert_subtype!({"primitive": "uuid"}, {"primitive": "string"});
        assert_not_subtype!({"primitive": "string"}, {"primitive": "uuid"});
    }

    #[test]
    fn test_subtype_entities() {
        let s = schema!({
            "entities": [
                {
                    "name": {"user": "Book"},
                    "idType": "uuid",
                    "fields": [],
                },
                {
                    "name": {"user": "Person"},
                    "idType": "uuid",
                    "fields": [],
                },
            ],
        });

        let id_book = type_!({"ref": [{"user": "Book"}, "id"]});
        let id_person = type_!({"ref": [{"user": "Person"}, "id"]});
        let eager_book = type_!({"ref": [{"user": "Book"}, "eager"]});
        let eager_person = type_!({"ref": [{"user": "Person"}, "eager"]});

        // the relation must be reflexive
        assert_subtype!(s, id_book, id_book, variant: Plain);
        assert_subtype!(s, id_book, id_book, variant: Entity);
        assert_subtype!(s, eager_book, eager_book, variant: Plain);
        assert_subtype!(s, eager_book, eager_book, variant: Entity);

        // references to different entities are never compatible
        assert_not_subtype!(s, id_book, id_person, variant: Plain);
        assert_not_subtype!(s, id_book, id_person, variant: Entity);
        assert_not_subtype!(s, eager_book, eager_person, variant: Plain);
        assert_not_subtype!(s, eager_book, eager_person, variant: Entity);

        // references of different kinds are equivalent in `Plain` typing...
        assert_subtype!(s, id_book, eager_book, variant: Plain);
        assert_subtype!(s, eager_book, id_book, variant: Plain);

        // ...but not in `Entity` typing
        assert_not_subtype!(s, id_book, eager_book, variant: Entity);
        assert_not_subtype!(s, eager_book, id_book, variant: Entity);
    }

    #[test]
    fn test_subtype_optional() {
        let string = type_!({"primitive": "string"});
        assert_subtype!(string, {"optional": string});
        assert_subtype!({"optional": string}, {"optional": string});
        assert_not_subtype!({"optional": string}, string);

        // nested optionals must be unwrapped
        assert_subtype!({"optional": {"optional": string}}, {"optional": string});
        assert_subtype!(string, {"optional": {"optional": string}});

        // typedefs must also be unwrapped
        let t = type_!({"typedef": type_name!("T")});
        let u = type_!({"typedef": type_name!("U")});
        let s = schema!({
            "entities": [],
            "typedefs": [
                [type_name!("T"), string],
                [type_name!("U"), {"optional": t}],
            ],
        });
        assert_subtype!(s, t, u);
        assert_not_subtype!(s, u, t);
        assert_subtype!(s, {"optional": string}, u);
    }

    #[test]
    fn test_subtype_array() {
        // arrays are covariant with respect to their element type
        let string = type_!({"primitive": "string"});
        let uuid = type_!({"primitive": "uuid"});
        assert_subtype!({"array": string}, {"array": string});
        assert_subtype!({"array": uuid}, {"array": string});
        assert_not_subtype!({"array": string}, {"array": uuid});
    }

    #[test]
    fn test_subtype_object() {
        let string = type_!({"primitive": "string"});
        let uuid = type_!({"primitive": "uuid"});

        // objects are covariant with respect to their field types
        let x_uuid = type_!({"object": {"fields": [
            {"name": "x", "type": uuid},
        ]}});
        let x_string = type_!({"object": {"fields": [
            {"name": "x", "type": string},
        ]}});
        assert_subtype!(x_uuid, x_string);
        assert_not_subtype!(x_string, x_uuid);

        // required field is a subtype of optional field
        let x_string_opt = type_!({"object": {"fields": [
            {"name": "x", "type": string, "optional": true},
        ]}});
        assert_subtype!(x_string, x_string_opt);
        assert_not_subtype!(x_string_opt, x_string);
        assert_subtype!(x_string_opt, x_string_opt);

        // missing field is a subtype of optional field
        let empty = type_!({"object": {"fields": []}});
        assert_subtype!(empty, x_string_opt);
        assert_not_subtype!(empty, x_string);
    }

    #[test]
    fn test_subtype_typedef() {
        let string = type_!({"primitive": "string"});
        let t = type_!({"typedef": type_name!("T")});
        let u = type_!({"typedef": type_name!("U")});
        let s = schema!({
            "entities": [],
            "typedefs": [
                [type_name!("T"), string],
                [type_name!("U"), {"array": string}],
            ],
        });

        assert_subtype!(s, string, t);
        assert_subtype!(s, t, string);
        assert_not_subtype!(s, string, u);
        assert_subtype!(s, {"array": string}, u);
    }

    #[test]
    fn test_subtype_recursive() {
        let string = type_!({"primitive": "string"});
        let uuid = type_!({"primitive": "uuid"});
        let l_str = type_!({"typedef": type_name!("Lstr")});
        let l_uuid = type_!({"typedef": type_name!("Luuid")});

        let s = schema!({
            "entities": [],
            "typedefs": [
                // Lstr = {head: string, tail: Lstr}
                [type_name!("Lstr"), {"object": {"fields": [
                    {"name": "head", "type": string},
                    {"name": "tail", "type": l_str},
                ]}}],

                // Luuid = {head: uuid, tail: Luuid}
                [type_name!("Luuid"), {"object": {"fields": [
                    {"name": "head", "type": uuid},
                    {"name": "tail", "type": l_uuid},
                ]}}],

            ],
        });

        // relation should be reflexive
        assert_subtype!(s, l_str, l_str);

        // Luuid is a subtype of Lstr
        assert_subtype!(s, l_uuid, l_str);
        assert_not_subtype!(s, l_str, l_uuid);
    }

    #[test]
    fn test_subtype_infinite() {
        let string = type_!({"primitive": "string"});
        let t = type_!({"typedef": type_name!("T")});
        let s = schema!({
            "entities": [],
            "typedefs": [
                // T = Array<T>
                [type_name!("T"), {"array": t}],
            ],
        });

        assert_subtype!(s, t, t);
        assert_subtype!(s, {"array": t}, t);
        assert_subtype!(s, t, {"array": t});
        assert_subtype!(s, {"array": {"array": t}}, t);
        assert_not_subtype!(s, {"array": string}, t);
        assert_not_subtype!(s, t, {"array": string});
    }

    #[test]
    fn test_optional() {
        let string = type_!({"primitive": "string"});
        let t = type_!({"typedef": type_name!("T")});
        let u = type_!({"typedef": type_name!("U")});
        let s = &schema!({
            "entities": [],
            "typedefs": [
                [type_name!("T"), string],
                [type_name!("U"), {"optional": t}],
            ],
        });

        assert_eq!(is_optional(s, &string), None);
        assert_eq!(is_optional(s, &type_!({"optional": string})), Some(&string));
        assert_eq!(is_optional(s, &type_!({"optional": {"optional": string}})), Some(&string));
        assert_eq!(is_optional(s, &t), None);
        assert_eq!(is_optional(s, &u), Some(&t));
        assert_eq!(is_optional(s, &type_!({"optional": u})), Some(&t));
        assert_eq!(is_optional(s, &type_!({"optional": t})), Some(&t));
    }

    #[test]
    fn test_selfref() {
        // invalid self-referential types
        let string = type_!({"primitive": "string"});
        let t = type_!({"typedef": type_name!("T")});
        let u = type_!({"typedef": type_name!("U")});
        let s = schema!({
            "entities": [],
            "typedefs": [
                // T = T
                [type_name!("T"), t],
                // U = U | undefined
                [type_name!("U"), {"optional": u}],
            ],
        });

        // type T is invalid, the algorithm behaves as if it was both subtype and supertype of
        // everything. this is not correct, but at least we check that the algorithms terminate.
        assert_subtype!(s, t, t);
        assert_subtype!(s, t, string);
        assert_subtype!(s, string, t);
        assert_eq!(is_optional(&s, &t), None);

        // type U behaves even more weird than type T; let's just check that the algorithm
        // terminates.
        assert_subtype!(s, u, u);
        assert_subtype!(s, u, {"optional": u});
        assert_not_subtype!(s, u, {"optional": string});
        assert_subtype!(s, {"optional": string}, u);
        assert_not_subtype!(s, u, string);
        assert_subtype!(s, string, u);
        assert_eq!(is_optional(&s, &t), None);
    }

}

use crate::chisel::{field_definition::IdMarker, AddTypeRequest, FieldDefinition};
use anyhow::{anyhow, bail, Result};
use std::collections::BTreeSet;
use std::path::Path;
use swc_common::sync::Lrc;
use swc_common::{
    errors::{emitter, Handler},
    SourceMap, Spanned,
};
use swc_ecma_ast::{
    ClassMember, ClassProp, Decl, Decorator, Expr, Ident, Lit, Stmt, TsEntityName,
    TsKeywordTypeKind, TsType, TsTypeAnn,
};
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, TsConfig};

fn swc_err<S: Spanned>(handler: &Handler, s: S, msg: &str) -> anyhow::Error {
    handler.span_err(s.span(), msg);
    anyhow!("{}", msg)
}

fn get_ident_string(handler: &Handler, x: &Expr) -> Result<String> {
    match x {
        Expr::Ident(id) => Ok(ident_to_string(id)),
        z => Err(swc_err(handler, z, "expected an identifier")),
    }
}

fn ident_to_string(id: &Ident) -> String {
    id.sym.to_string()
}

fn get_field_info(handler: &Handler, x: &Expr) -> Result<(String, bool)> {
    match x {
        Expr::Ident(id) => Ok((ident_to_string(id), id.optional)),
        z => Err(swc_err(handler, z, "expected an identifier")),
    }
}

fn type_to_string(handler: &Handler, x: &TsType) -> Result<String> {
    match x {
        TsType::TsKeywordType(kw) => match kw.kind {
            TsKeywordTypeKind::TsBigIntKeyword => Ok("bigint".into()),
            TsKeywordTypeKind::TsStringKeyword => Ok("string".into()),
            TsKeywordTypeKind::TsNumberKeyword => Ok("number".into()),
            TsKeywordTypeKind::TsBooleanKeyword => Ok("boolean".into()),
            _ => Err(swc_err(handler, x, "type keyword not supported")),
        },
        TsType::TsTypeRef(tr) => match &tr.type_name {
            TsEntityName::Ident(id) => Ok(ident_to_string(id)),
            TsEntityName::TsQualifiedName(_) => Err(anyhow!("qualified names not supported")),
        },
        TsType::TsArrayType(arr) => {
            Ok("Array[".to_string() + &type_to_string(handler, &arr.elem_type)? + "]")
        }
        TsType::TsOptionalType(opt) => Ok(type_to_string(handler, &opt.type_ann)? + "?"),
        t => Err(swc_err(handler, t, "type not supported")),
    }
}

fn get_field_type(handler: &Handler, x: &Option<TsTypeAnn>) -> Result<String> {
    let t = x
        .clone()
        .ok_or_else(|| anyhow!("type ann temporarily mandatory"))?;

    type_to_string(handler, &t.type_ann)
}

fn lit_to_string(handler: &Handler, x: &Lit) -> Result<String> {
    match x {
        Lit::Str(x) => Ok(x.value.to_string()),
        Lit::Bool(x) => Ok(x.value.to_string()),
        Lit::Num(x) => Ok(x.value.to_string()),
        Lit::BigInt(x) => Ok(x.value.to_string()),
        x => Err(swc_err(handler, x, "literal not supported")),
    }
}

fn lit_name(x: &Lit) -> &str {
    match x {
        Lit::Str(_) => "string",
        Lit::Bool(_) => "boolean",
        Lit::Num(_) => "number",
        Lit::BigInt(_) => "bigint",
        Lit::Null(_) => "null",
        Lit::JSXText(_) => "JSXText",
        Lit::Regex(_) => "regex",
    }
}

fn get_field_value(handler: &Handler, x: &Option<Box<Expr>>) -> Result<Option<(String, String)>> {
    match x {
        None => Ok(None),
        Some(k) => match &**k {
            Expr::Lit(k) => {
                let val = lit_to_string(handler, k)?;
                let val_type = lit_name(k).into();
                Ok(Some((val, val_type)))
            }
            x => Err(swc_err(handler, x, "expression not supported")),
        },
    }
}

fn get_type_decorators(handler: &Handler, x: &[Decorator]) -> Result<BTreeSet<String>> {
    let mut output = BTreeSet::new();
    for dec in x.iter() {
        output.insert(get_ident_string(handler, &dec.expr)?);
    }
    Ok(output)
}

fn validate_id(marker: &IdMarker, ty: &str, field_name: &str) -> Result<()> {
    if *marker != IdMarker::Uuid
        && *marker != IdMarker::AutoIncrement
        && field_name.to_lowercase() == "id"
    {
        anyhow::bail!("You are creating a field named id, but not adding an id decorator (@id, or @uuid). Add a decorator or rename the field");
    }

    if *marker != IdMarker::None {
        anyhow::ensure!(
            ty != "number",
            "the type number cannot be used with an ID decorator"
        );

        anyhow::ensure!(
            ty != "boolean",
            "the type boolean cannot be used with an ID decorator"
        );

        if *marker == IdMarker::Uuid {
            anyhow::ensure!(ty == "string", "only string can be marked as Uuid");
        }

        if *marker == IdMarker::AutoIncrement {
            anyhow::ensure!(ty == "bigint", "ids have to be bigint");
        }
    }
    Ok(())
}

fn validate_type_vec(type_vec: &[AddTypeRequest], valid_types: &BTreeSet<String>) -> Result<()> {
    let mut basic_types: BTreeSet<&str> = BTreeSet::new();
    basic_types.insert("string");
    basic_types.insert("number");
    basic_types.insert("bigint");
    basic_types.insert("boolean");

    for t in type_vec {
        for field in t.field_defs.iter() {
            if basic_types.get(&field.field_type as &str).is_none()
                && valid_types.get(&field.field_type).is_none()
            {
                bail!("field {} in class {} neither a basic type, nor refers to a type defined in this context",
                                   field.name, t.name
                                   );
            }
        }
    }
    Ok(())
}

impl From<&str> for IdMarker {
    fn from(val: &str) -> Self {
        match val {
            "unique" => IdMarker::Unique,
            "uuid" => IdMarker::Uuid,
            "id" => IdMarker::AutoIncrement,
            _ => unreachable!(),
        }
    }
}

fn parse_class_prop(x: &ClassProp, handler: &Handler) -> Result<FieldDefinition> {
    let (default_value, field_type) = match get_field_value(handler, &x.value)? {
        None => (None, get_field_type(handler, &x.type_ann)?),
        Some((val, t)) => (Some(val), t),
    };
    let (field_name, is_optional) = get_field_info(handler, &x.key)?;

    let mut labels = get_type_decorators(handler, &x.decorators)?;
    let mut ids = vec![];
    for id_type in ["unique", "uuid", "id"].into_iter() {
        if labels.remove(id_type) {
            ids.push(id_type);
        }
    }
    let id_marker = if ids.is_empty() {
        IdMarker::None
    } else if ids.len() == 1 {
        IdMarker::from(ids[0])
    } else {
        bail!("Impossible condition! Ids array was constructed explicitly, but seems corrupted");
    };
    validate_id(&id_marker, &field_type, &field_name)?;

    Ok(FieldDefinition {
        name: field_name,
        is_optional,
        default_value,
        field_type,
        labels: labels.iter().cloned().collect(),
        id_marker: id_marker.into(),
    })
}

fn parse_one_file<P: AsRef<Path>>(
    filename: &P,
    type_vec: &mut Vec<AddTypeRequest>,
    valid_types: &mut BTreeSet<String>,
) -> Result<()> {
    let cm: Lrc<SourceMap> = Default::default();

    let emitter = Box::new(emitter::EmitterWriter::new(
        Box::new(std::io::stderr()),
        Some(cm.clone()),
        false,
        true,
    ));

    let handler = Handler::with_emitter(true, false, emitter);

    let fm = cm.load_file(filename.as_ref())?;

    let mut config = TsConfig {
        decorators: true,
        ..Default::default()
    };
    config.decorators = true;

    let lexer = Lexer::new(
        // We want to parse typescript with decorators support
        Syntax::Typescript(config),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );

    let mut parser = Parser::new_from(lexer);
    let mut errors = false;
    for e in parser.take_errors() {
        errors = true;
        e.into_diagnostic(&handler).emit();
    }
    if errors {
        bail!("Exiting on parsing errors");
    }

    let x = parser.parse_script().map_err(|e| {
        e.into_diagnostic(&handler).emit();
        anyhow!("Exiting on script parsing errors")
    })?;

    for decl in &x.body {
        match decl {
            Stmt::Decl(Decl::Class(x)) => {
                let mut field_defs = Vec::default();
                let name = ident_to_string(&x.ident);
                if !valid_types.insert(name.clone()) {
                    bail!("Type {} defined twice", name);
                }

                let mut ids = 0;
                for member in &x.class.body {
                    match member {
                        ClassMember::ClassProp(x) => match parse_class_prop(x, &handler) {
                            Err(err) => {
                                handler
                                    .span_err(x.span(), &format!("While parsing class {}", name));
                                bail!("{}", err);
                            }
                            Ok(fd) => {
                                if fd.id_marker == IdMarker::AutoIncrement as i32 {
                                    ids += 1;
                                }
                                field_defs.push(fd);
                            }
                        },
                        z => {
                            handler.span_err(z.span(), "Only property definitions (with optional decorators) allowed in the types file");
                            bail!("invalid type file {}", filename.as_ref().display());
                        }
                    }
                }
                if ids > 1 {
                    handler.span_err(x.span(), &format!("While parsing class {}", name));
                    bail!("Only one @id supported per class");
                }

                type_vec.push(AddTypeRequest { name, field_defs });
            }
            z => {
                handler.span_err(z.span(), "Only property definitions (with optional decorators) allowed in the types file");
                bail!("invalid type file {}", filename.as_ref().display());
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_types<P: AsRef<Path>>(files: &[P]) -> Result<Vec<AddTypeRequest>> {
    let mut type_vec = vec![];

    let mut valid_types = BTreeSet::new();

    for filename in files {
        parse_one_file(filename, &mut type_vec, &mut valid_types)?;
    }

    validate_type_vec(&type_vec, &valid_types)?;
    Ok(type_vec)
}

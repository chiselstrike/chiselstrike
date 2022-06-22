use crate::chisel::{type_msg::TypeEnum, AddTypeRequest, ContainerType, FieldDefinition, TypeMsg};
use anyhow::{anyhow, bail, ensure, Context, Result};
use chisel_server::auth::is_auth_entity_name;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use swc_common::sync::Lrc;
use swc_common::{
    errors::{emitter, Handler},
    SourceMap, Spanned,
};
use swc_ecma_ast::PropName;
use swc_ecma_ast::{
    ClassMember, ClassProp, Decl, Decorator, Expr, Ident, Lit, ModuleDecl, ModuleItem,
    TsEntityName, TsKeywordTypeKind, TsType, TsTypeAnn,
};
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, TsConfig};
use swc_ecmascript::ast as swc_ecma_ast;
use swc_ecmascript::parser as swc_ecma_parser;

impl FieldDefinition {
    pub(crate) fn field_type(&self) -> Result<&TypeEnum> {
        self.field_type
            .as_ref()
            .with_context(|| format!("field_type of field '{}' is None", self.name))?
            .type_enum
            .as_ref()
            .with_context(|| format!("type_enum of field '{}' is None", self.name))
    }
}

impl ContainerType {
    fn value_type(&self) -> Result<&TypeEnum> {
        self.value_type
            .as_ref()
            .context("value_type of ContainerType is None")?
            .type_enum
            .as_ref()
            .context("type_enum of value_type of ContainerType is None")
    }
}

impl TypeEnum {
    fn list(inner: TypeEnum) -> Self {
        let inner = TypeMsg {
            type_enum: Some(inner),
        };
        TypeEnum::List(Box::new(ContainerType {
            value_type: Some(Box::new(inner)),
        }))
    }
}

impl fmt::Display for TypeEnum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TypeEnum::String(_) => "string".to_string(),
            TypeEnum::Number(_) => "number".to_string(),
            TypeEnum::Bool(_) => "bool".to_string(),
            TypeEnum::Entity(name) => name.to_string(),
            TypeEnum::List(inner) => {
                let inner = inner.value_type();
                format!("List<{:#?}>", inner)
            }
        };
        write!(f, "{s}")
    }
}

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

fn get_field_info(handler: &Handler, x: &PropName) -> Result<(String, bool)> {
    match x {
        PropName::Ident(id) => Ok((ident_to_string(id), id.optional)),
        z => Err(swc_err(handler, z, "expected an identifier")),
    }
}

fn map_type(handler: &Handler, x: &TsType) -> Result<TypeEnum> {
    match x {
        TsType::TsKeywordType(kw) => match kw.kind {
            TsKeywordTypeKind::TsStringKeyword => Ok(TypeEnum::String(true)),
            TsKeywordTypeKind::TsNumberKeyword => Ok(TypeEnum::Number(true)),
            TsKeywordTypeKind::TsBooleanKeyword => Ok(TypeEnum::Bool(true)),
            _ => Err(swc_err(handler, x, "type keyword not supported")),
        },
        TsType::TsTypeRef(tr) => match &tr.type_name {
            TsEntityName::Ident(id) => {
                let ident_name = ident_to_string(id);
                if ident_name == "List" {
                    let type_params = tr.type_params.as_ref().ok_or_else(|| {
                        swc_err(handler, tr, "type params must be set for List type")
                    })?;
                    if type_params.params.len() != 1 {
                        anyhow::bail!(
                            "unexpected number of List type params. expected 1, got {}",
                            type_params.params.len()
                        );
                    }
                    let inner_type = map_type(handler, &type_params.params[0])?;
                    Ok(TypeEnum::list(inner_type))
                } else {
                    Ok(TypeEnum::Entity(ident_name))
                }
            }
            TsEntityName::TsQualifiedName(_) => Err(anyhow!("qualified names are not supported")),
        },
        t => Err(swc_err(handler, t, "type is not supported")),
    }
}

fn get_field_type(handler: &Handler, x: &Option<TsTypeAnn>) -> Result<TypeEnum> {
    let t = x
        .clone()
        .context("type annotation is temporarily mandatory")?;
    map_type(handler, &t.type_ann)
}

fn parse_literal(handler: &Handler, x: &Lit) -> Result<(String, TypeEnum)> {
    let r = match x {
        Lit::Str(x) => (x.value.to_string(), TypeEnum::String(true)),
        Lit::Bool(x) => (x.value.to_string(), TypeEnum::Bool(true)),
        Lit::Num(x) => (x.value.to_string(), TypeEnum::Number(true)),
        x => anyhow::bail!(swc_err(handler, x, "literal not supported")),
    };
    Ok(r)
}

fn get_field_value(handler: &Handler, x: &Option<Box<Expr>>) -> Result<Option<(String, TypeEnum)>> {
    match x {
        None => Ok(None),
        Some(k) => match &**k {
            Expr::Lit(k) => {
                let (val, val_type) = parse_literal(handler, k)?;
                Ok(Some((val, val_type)))
            }
            Expr::Unary(k) => {
                let op = k.op;
                let value = get_field_value(handler, &Some(k.arg.clone()))?
                    .ok_or_else(|| swc_err(handler, k, "unexpected empty expression"))?;
                Ok(Some((format!("{}{}", op, value.0), value.1)))
            }
            // Covers entity fields initialization.
            Expr::New(_) => Ok(None),
            x => Err(swc_err(handler, x, "expression not supported")),
        },
    }
}

fn get_type_decorators(handler: &Handler, x: &[Decorator]) -> Result<(Vec<String>, bool)> {
    let mut output = vec![];
    let mut is_unique = false;
    for dec in x.iter() {
        match &*dec.expr {
            Expr::Call(call) => {
                let callee = call.callee.clone().expr().ok_or_else(|| {
                    anyhow!("expected expression, got {:?} instead", call.callee.clone())
                })?;
                let name = get_ident_string(handler, &callee)?;
                ensure!(
                    name == "labels",
                    format!("decorator '{}' is not supported by ChiselStrike", name)
                );
                for arg in &call.args {
                    if let Some((label, ty)) = get_field_value(handler, &Some(arg.expr.clone()))? {
                        ensure!(
                            matches!(ty, TypeEnum::String(_)),
                            "Only strings accepted as labels"
                        );
                        output.push(label);
                    }
                }
            }
            Expr::Ident(x) => {
                let name = ident_to_string(x);
                ensure!(name != "labels", "expected a call-like decorator");

                ensure!(
                    name == "unique",
                    format!("decorator '{}' is not supported by ChiselStrike", name)
                );
                is_unique = true;
            }
            z => {
                return Err(swc_err(handler, z, "expected a call-like decorator"));
            }
        };
    }
    Ok((output, is_unique))
}

fn validate_type_vec(type_vec: &[AddTypeRequest], valid_entities: &BTreeSet<String>) -> Result<()> {
    for t in type_vec {
        for field in t.field_defs.iter() {
            let field_type = field.field_type.as_ref().unwrap();
            validate_type(field_type, valid_entities).with_context(|| {
                format!(
                    "field '{}' in class '{}' contains invalid type",
                    field.name, t.name
                )
            })?;
        }
    }
    Ok(())
}

fn validate_type(ty: &TypeMsg, valid_entities: &BTreeSet<String>) -> Result<()> {
    match ty.type_enum.as_ref().unwrap() {
        TypeEnum::Entity(name) => {
            if valid_entities.get(name).is_none() && !is_auth_entity_name(name) {
                bail!("entity of name '{name}' is undefined");
            }
        }
        TypeEnum::List(inner) => validate_type(inner.value_type.as_ref().unwrap(), valid_entities)?,
        _ => (),
    }
    Ok(())
}

fn parse_class_prop(x: &ClassProp, class_name: &str, handler: &Handler) -> Result<FieldDefinition> {
    let (field_name, is_optional) = get_field_info(handler, &x.key)?;
    anyhow::ensure!(field_name != "id", "Creating a field with the name `id` is not supported. 😟\nBut don't worry! ChiselStrike creates an id field automatically, and you can access it in your endpoints as {}.id 🤩", class_name);

    let (default_value, field_type) = match get_field_value(handler, &x.value)? {
        None => (None, get_field_type(handler, &x.type_ann)?),
        Some((val, val_ty)) => (Some(val), val_ty),
    };
    let (labels, is_unique) = get_type_decorators(handler, &x.decorators)?;

    match &field_type {
        TypeEnum::Entity(name) if !is_optional => match &x.value {
            None => {
                eprintln!(
                        "Warning: Entity `{class_name}` contains field `{field_name}` of entity type `{name}` which is not default-initialized.\n\
                        \tWhen using this field, its methods might not be available. As a temporary workaround, please consider initializing the field `{field_name}: {name} = new {name}();`\n\
                        \tFor further information, please see https://github.com/chiselstrike/chiselstrike/issues/1541"
                    );
            }
            Some(k) => match &**k {
                Expr::New(_) => {}
                x => anyhow::bail!(swc_err(
                    handler,
                    x,
                    &format!(
                        "field `{field_name}` of entity type `{name}` has unexpected initializer"
                    ),
                )),
            },
        },
        _ => {}
    };

    Ok(FieldDefinition {
        name: field_name,
        is_optional,
        is_unique,
        default_value,
        field_type: Some(TypeMsg {
            type_enum: field_type.into(),
        }),
        labels,
    })
}

fn parse_class_decl<P: AsRef<Path>>(
    handler: &Handler,
    filename: &P,
    type_vec: &mut Vec<AddTypeRequest>,
    valid_types: &mut BTreeSet<String>,
    decl: &Decl,
) -> Result<()> {
    match decl {
        Decl::Class(x) => {
            let mut field_defs = Vec::default();
            let name = ident_to_string(&x.ident);
            if !valid_types.insert(name.clone()) {
                bail!("Model {} defined twice", name);
            }

            for member in &x.class.body {
                match member {
                    ClassMember::ClassProp(x) => match parse_class_prop(x, &name, handler) {
                        Err(err) => {
                            handler.span_err(x.span(), &format!("While parsing class {}", name));
                            bail!("{}", err);
                        }
                        Ok(fd) => {
                            field_defs.push(fd);
                        }
                    },
                    ClassMember::Constructor(_x) => {
                        handler.span_err(member.span(), "Constructors not allowed in ChiselStrike model definitions. Consider adding default values so one is not needed, or call ChiselEntity's create method");
                        bail!("invalid type file {}", filename.as_ref().display());
                    }
                    _ => {}
                }
            }
            type_vec.push(AddTypeRequest { name, field_defs });
        }
        z => {
            handler.span_err(z.span(), "Only class definitions allowed in the types file");
            bail!("invalid type file {}", filename.as_ref().display());
        }
    }
    Ok(())
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

    let x = parser.parse_typescript_module().map_err(|e| {
        e.into_diagnostic(&handler).emit();
        anyhow!("Exiting on script parsing errors")
    })?;

    for decl in &x.body {
        match decl {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(exp)) => {
                parse_class_decl(&handler, filename, type_vec, valid_types, &exp.decl)?;
            }
            ModuleItem::ModuleDecl(ModuleDecl::Import(_)) => {
                // Right now just accept imports, but don't try to parse them.
                // The compiler will error out if the imports are invalid.
            }
            z => {
                handler.span_err(
                    z.span(),
                    "ChiselStrike expects either import statements or exported classes (but not default exported)",
                );
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

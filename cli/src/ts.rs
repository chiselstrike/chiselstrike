use crate::chisel::{AddTypeRequest, FieldDefinition};
use anyhow::{anyhow, bail, ensure, Context, Result};
use compile::swc_common;
use compile::swc_ecmascript;
use std::collections::BTreeSet;
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

fn type_to_string(handler: &Handler, x: &TsType) -> Result<String> {
    match x {
        TsType::TsKeywordType(kw) => match kw.kind {
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
    let t = x.clone().context("type ann temporarily mandatory")?;

    type_to_string(handler, &t.type_ann)
}

fn lit_to_string(handler: &Handler, x: &Lit) -> Result<String> {
    match x {
        Lit::Str(x) => Ok(x.value.to_string()),
        Lit::Bool(x) => Ok(x.value.to_string()),
        Lit::Num(x) => Ok(x.value.to_string()),
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
            Expr::Unary(k) => {
                let op = k.op;
                let value = get_field_value(handler, &Some(k.arg.clone()))?
                    .ok_or_else(|| swc_err(handler, k, "unexpected empty expression"))?;
                Ok(Some((format!("{}{}", op, value.0), value.1)))
            }
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
                        ensure!(ty == "string", "Only strings accepted as labels");
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

fn validate_type_vec(type_vec: &[AddTypeRequest], valid_types: &BTreeSet<String>) -> Result<()> {
    let mut builtin_types: BTreeSet<&str> = BTreeSet::new();
    builtin_types.insert("string");
    builtin_types.insert("number");
    builtin_types.insert("boolean");
    builtin_types.insert("AuthUser");

    for t in type_vec {
        for field in t.field_defs.iter() {
            if builtin_types.get(&field.field_type as &str).is_none()
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

fn parse_class_prop(x: &ClassProp, class_name: &str, handler: &Handler) -> Result<FieldDefinition> {
    let (default_value, field_type) = match get_field_value(handler, &x.value)? {
        None => (None, get_field_type(handler, &x.type_ann)?),
        Some((val, t)) => (Some(val), t),
    };
    let (field_name, is_optional) = get_field_info(handler, &x.key)?;

    anyhow::ensure!(field_name != "id", "Creating a field with the name `id` is not supported. 😟\nBut don't worry! ChiselStrike creates an id field automatically, and you can access it in your endpoints as {}.id 🤩", class_name);

    let (labels, is_unique) = get_type_decorators(handler, &x.decorators)?;

    Ok(FieldDefinition {
        name: field_name,
        is_optional,
        is_unique,
        default_value,
        field_type,
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

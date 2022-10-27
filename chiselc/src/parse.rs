// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, Result};
use std::io::Write;
use std::sync::Arc;
use swc_common::{
    errors::{emitter, Handler},
    source_map::FileName,
    sync::Lrc,
    Globals, Mark, SourceMap, GLOBALS,
};
use swc_ecmascript::parser::{lexer::Lexer, Parser, StringInput, Syntax};
use swc_ecmascript::visit::{FoldWith, VisitMut};
use swc_ecmascript::{ast::Module, transforms::resolver};
use swc_ecmascript::{
    codegen::{text_writer::JsWriter, Emitter},
    transforms::hygiene,
};

use crate::rewrite::{Rewriter, Target};
use crate::symbols::Symbols;

#[derive(Clone, Default)]
struct ErrorBuffer {
    inner: Arc<std::sync::Mutex<Vec<u8>>>,
}

impl ErrorBuffer {
    fn get(&self) -> String {
        String::from_utf8_lossy(&self.inner.lock().unwrap().clone()).to_string()
    }
}

impl std::io::Write for ErrorBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut v = self.inner.lock().unwrap();
        v.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn canonical_transforms(module: &mut Module) {
    let globals = Globals::new();
    GLOBALS.set(&globals, || {
        let mut resolver = resolver(Mark::new(), Mark::new(), true);
        resolver.visit_mut_module(module);

        let mut hygiene = hygiene();
        hygiene.visit_mut_module(module);
    })
}

#[derive(Default)]
pub struct ParserContext {
    pub sm: Lrc<SourceMap>,
    error_buffer: ErrorBuffer,
}

impl ParserContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(&self, code: String, apply_transforms: bool) -> Result<Module> {
        let emitter = Box::new(emitter::EmitterWriter::new(
            Box::new(self.error_buffer.clone()),
            Some(self.sm.clone()),
            false,
            true,
        ));
        let handler = Handler::with_emitter(true, false, emitter);
        let fm = self.sm.new_source_file(FileName::Anon, code);
        let config = swc_ecmascript::parser::TsConfig {
            decorators: true,
            ..Default::default()
        };
        let lexer = Lexer::new(
            Syntax::Typescript(config),
            Default::default(),
            StringInput::from(&*fm),
            None,
        );

        let mut parser = Parser::new_from(lexer);

        for e in parser.take_errors() {
            e.into_diagnostic(&handler).emit();
        }

        let mut module = parser.parse_typescript_module().map_err(|e| {
            // Unrecoverable fatal error occurred
            e.into_diagnostic(&handler).emit();
            anyhow!("Parse failed: {}", self.error_buffer.get())
        })?;

        if apply_transforms {
            canonical_transforms(&mut module);
        }

        Ok(module)
    }
}

pub fn compile<W: Write>(
    code: String,
    symbols: Symbols,
    target: Target,
    mut output: W,
) -> Result<()> {
    let ctx = ParserContext::new();
    // FIXME: We probably need a name for better error messages.
    let module = ctx.parse(code, false)?;

    let mut rewriter = Rewriter::new(symbols);
    let module = rewriter.rewrite(module);

    if let Target::FilterProperties = target {
        writeln!(&mut output, "{}", serde_json::to_string(&rewriter.indexes)?)?;
    }

    emit(module, target, ctx.sm, output)
}

pub fn emit<W: Write>(
    module: Module,
    target: Target,
    sm: Lrc<SourceMap>,
    mut output: W,
) -> Result<()> {
    // If we're emitting JavaScript, get rid of TypeScript types.
    let module = match target {
        Target::JavaScript => {
            let globals = Globals::default();
            GLOBALS.set(&globals, || {
                let top_level_mark = Mark::fresh(Mark::root());
                module.fold_with(&mut swc_ecmascript::transforms::typescript::strip(
                    top_level_mark,
                ))
            })
        }
        _ => module,
    };
    // Emit the final output, depending on the target.
    match target {
        Target::JavaScript | Target::TypeScript => {
            let mut emitter = Emitter {
                cfg: swc_ecmascript::codegen::Config {
                    ..Default::default()
                },
                cm: sm.clone(),
                comments: None,
                wr: JsWriter::new(sm, "\n", &mut output, None),
            };
            emitter.emit_module(&module).unwrap();
        }
        _ => (),
    }
    Ok(())
}

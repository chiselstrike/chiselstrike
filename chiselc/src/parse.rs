use anyhow::{anyhow, Result};
use std::io::Write;
use std::sync::Arc;
use swc_common::{
    errors::{emitter, Handler},
    source_map::FileName,
    sync::Lrc,
    Globals, Mark, SourceMap, GLOBALS,
};
use swc_ecmascript::ast::Module;
use swc_ecmascript::codegen::{text_writer::JsWriter, Emitter};
use swc_ecmascript::parser::{lexer::Lexer, Parser, StringInput, Syntax};
use swc_ecmascript::visit::FoldWith;

use crate::{
    rewrite::{Rewriter, Target},
    symbols::Symbols,
};

#[derive(Clone)]
struct ErrorBuffer {
    inner: Arc<std::sync::Mutex<Vec<u8>>>,
}

impl ErrorBuffer {
    fn new() -> Self {
        Self {
            inner: Arc::new(std::sync::Mutex::new(vec![])),
        }
    }

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

pub fn parse(code: String, sm: &Lrc<SourceMap>) -> Result<Module> {
    let err_buf = ErrorBuffer::new();
    let emitter = Box::new(emitter::EmitterWriter::new(
        Box::new(err_buf.clone()),
        Some(sm.clone()),
        false,
        true,
    ));
    let handler = Handler::with_emitter(true, false, emitter);
    let fm = sm.new_source_file(FileName::Anon, code);
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

    let module = parser.parse_typescript_module().map_err(|e| {
        // Unrecoverable fatal error occurred
        e.into_diagnostic(&handler).emit();
        anyhow!("Parse failed: {}", err_buf.get())
    })?;

    Ok(module)
}

pub fn compile(
    code: String,
    symbols: Symbols,
    target: Target,
    mut output: Box<dyn Write>,
) -> Result<()> {
    let sm: Lrc<SourceMap> = Default::default();
    // FIXME: We probably need a name for better error messages.
    let module = parse(code, &sm)?;

    let mut rewriter = Rewriter::new(symbols);
    let module = rewriter.rewrite(module);
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
        Target::FilterProperties => {
            println!("{}", serde_json::to_string(&rewriter.indexes)?);
        }
    }
    Ok(())
}

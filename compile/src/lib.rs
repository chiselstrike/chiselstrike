// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, Result};
use std::sync::Arc;
use swc_common::{
    errors::{emitter, Handler},
    source_map::FileName,
    sync::Lrc,
    SourceMap,
};
use swc_ecma_codegen::{text_writer::JsWriter, Emitter};
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax};
use swc_ecma_visit::FoldWith;

pub fn compile_ts_code(code: String) -> Result<String> {
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

    let err_buf = ErrorBuffer::new();

    let cm: Lrc<SourceMap> = Default::default();
    let emitter = Box::new(emitter::EmitterWriter::new(
        Box::new(err_buf.clone()),
        Some(cm.clone()),
        false,
        true,
    ));
    let handler = Handler::with_emitter(true, false, emitter);

    // FIXME: We probably need a name for better error messages.
    let fm = cm.new_source_file(FileName::Anon, code);
    let config = swc_ecma_parser::TsConfig {
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

    // Remove typescript types
    let module = module.fold_with(&mut swc_ecma_transforms_typescript::strip());

    let mut buf = vec![];
    {
        let mut emitter = Emitter {
            cfg: swc_ecma_codegen::Config {
                ..Default::default()
            },
            cm: cm.clone(),
            comments: None,
            wr: JsWriter::new(cm, "\n", &mut buf, None),
        };
        emitter.emit_module(&module).unwrap();
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

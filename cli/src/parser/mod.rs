pub mod ast;
pub mod tokenizer;

use ast::{FieldDef, TypeDef, TypeSystemDef};
use std::error::Error;
use std::fmt;
use tokenizer::{Token, Tokenizer, TokenizerError};

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    TokenizerError(String),
    ParserError(String),
}

impl Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "parse error: {}",
            match self {
                ParseError::TokenizerError(s) => s,
                ParseError::ParserError(s) => s,
            }
        )
    }
}

impl From<TokenizerError> for ParseError {
    fn from(e: TokenizerError) -> Self {
        ParseError::TokenizerError(format!(
            "{} at line: {}, column: {}",
            e.message, e.line, e.column
        ))
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    token_idx: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            token_idx: 0,
        }
    }

    pub fn is_eof(&self) -> bool {
        matches!(self.tokens.get(self.token_idx), Some(Token::Eof))
    }

    pub fn parse_type_def(&mut self) -> Result<TypeDef, ParseError> {
        match self.next_token() {
            Token::Name(ident) if ident == "type" => self.parse_type_definition(),
            token => Err(ParseError::ParserError(format!(
                "unregonized token {:?}",
                token
            ))),
        }
    }

    pub fn parse_type_definition(&mut self) -> Result<TypeDef, ParseError> {
        let object_name = self.parse_name()?;
        let mut fields = Vec::new();
        self.expect_token(Token::Punctuator('{'))?;
        while self.peek_token() != Token::Punctuator('}') {
            let field_name = self.parse_name()?;
            self.expect_token(Token::Punctuator(':'))?;
            let field_type = self.parse_name()?;
            let field = FieldDef::new(field_name, field_type);
            fields.push(field);
        }
        self.expect_token(Token::Punctuator('}'))?;
        Ok(TypeDef {
            name: object_name,
            fields,
        })
    }

    pub fn parse_name(&mut self) -> Result<String, ParseError> {
        match self.next_token() {
            Token::Name(name) => Ok(name),
            unexpected => self.expected("name", unexpected),
        }
    }

    pub fn expect_token(&mut self, expected: Token) -> Result<(), ParseError> {
        let found = self.next_token();
        if found != expected {
            return Err(ParseError::ParserError(format!(
                "expected {:?}, found {:?}",
                expected, found
            )));
        }
        Ok(())
    }

    pub fn expected<T>(&self, expected: &str, found: Token) -> Result<T, ParseError> {
        Err(ParseError::ParserError(format!(
            "expected {}, found {:?}",
            expected, found
        )))
    }

    pub fn next_token(&mut self) -> Token {
        self.token_idx += 1;
        match self.tokens.get(self.token_idx - 1) {
            Some(token) => token.to_owned(),
            None => Token::Eof,
        }
    }

    pub fn peek_token(&mut self) -> Token {
        match self.tokens.get(self.token_idx) {
            Some(token) => token.to_owned(),
            None => Token::Eof,
        }
    }
}

pub fn parse(raw: &str) -> Result<TypeSystemDef, ParseError> {
    let mut tokenizer = Tokenizer::default();
    let tokens = tokenizer.tokenize(raw.to_string())?;
    let mut parser = Parser::new(tokens);
    let mut defs = Vec::new();
    while !parser.is_eof() {
        let def = parser.parse_type_def()?;
        defs.push(def);
    }
    Ok(TypeSystemDef::new(defs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_type_definition() {
        let doc = String::from("type Person { name: String }");
        let mut tokenizer = Tokenizer::default();
        let tokens = tokenizer.tokenize(doc.to_string()).unwrap();
        let mut parser = Parser::new(tokens);
        let actual = parser.parse_type_def().unwrap();
        let expected = TypeDef {
            name: "Person".to_string(),
            fields: vec![FieldDef::new("name".to_string(), "String".to_string())],
        };
        assert_eq!(expected, actual);
    }
}

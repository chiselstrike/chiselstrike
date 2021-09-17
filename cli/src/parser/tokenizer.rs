use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Token {
    Eof,
    Punctuator(char),
    Name(String),
    IntValue,
    FloatValue,
    StringValue(String),
}

impl Token {
    pub fn punctuator(ch: char) -> Token {
        Token::Punctuator(ch)
    }

    pub fn keyword(word: &str) -> Token {
        Token::Name(word.to_string())
    }

    pub fn name(name: String) -> Token {
        Token::Name(name)
    }

    pub fn string_value(value: String) -> Token {
        Token::StringValue(value)
    }
}

#[derive(Debug, PartialEq)]
pub struct TokenizerError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl TokenizerError {
    pub fn new(message: String, line: usize, column: usize) -> Self {
        Self {
            message,
            line,
            column,
        }
    }
}

pub struct Tokenizer {
    pub line: usize,
    pub column: usize,
}

impl Default for Tokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Tokenizer {
    pub fn new() -> Self {
        Self { line: 1, column: 1 }
    }

    pub fn tokenize(&mut self, doc: String) -> Result<Vec<Token>, TokenizerError> {
        let mut ret: Vec<Token> = vec![];
        let mut peekable = doc.chars().peekable();
        loop {
            if let Some(token) = self.next_token(&mut peekable)? {
                ret.push(token.to_owned());
                if token == Token::Eof {
                    break;
                }
            }
        }
        Ok(ret)
    }

    fn next_token(
        &mut self,
        chars: &mut Peekable<Chars<'_>>,
    ) -> Result<Option<Token>, TokenizerError> {
        match chars.peek() {
            Some(&ch) => match ch {
                '{' | '}' | '(' | ')' | ':' => Ok(Some(self.consume_punctuator(ch, chars))),
                '\"' => Ok(Some(self.consume_string(chars))),
                ch if Tokenizer::is_name_start(ch) => Ok(Some(self.consume_name(chars))),
                _ if Tokenizer::is_whitespace(ch) => {
                    self.consume_whitespace(chars);
                    Ok(None)
                }
                _ => Err(TokenizerError::new(
                    format!("unrecognized character: `{}`", ch),
                    self.line,
                    self.column,
                )),
            },
            None => Ok(Some(Token::Eof)),
        }
    }

    fn consume_punctuator(&mut self, ch: char, chars: &mut Peekable<Chars<'_>>) -> Token {
        let ret = Token::punctuator(ch);
        chars.next();
        self.column += 1;
        ret
    }

    fn consume_string(&mut self, chars: &mut Peekable<Chars<'_>>) -> Token {
        chars.next();
        self.column += 1;
        let mut ret = String::new();
        while let Some(&ch) = chars.peek() {
            chars.next();
            self.column += 1;
            if ch == '\"' {
                break;
            }
            ret.push(ch);
        }
        Token::string_value(ret)
    }

    fn consume_name(&mut self, chars: &mut Peekable<Chars<'_>>) -> Token {
        let mut ret = String::new();
        while let Some(&ch) = chars.peek() {
            if Tokenizer::is_name_continue(ch) {
                chars.next();
                self.column += 1;
                ret.push(ch);
            } else {
                break;
            }
        }
        Token::name(ret)
    }

    fn consume_whitespace(&mut self, chars: &mut Peekable<Chars<'_>>) {
        while let Some(&ch) = chars.peek() {
            if Tokenizer::is_whitespace(ch) {
                if ch == '\n' {
                    self.line += 1;
                    self.column = 1;
                } else {
                    self.column += 1;
                }
                chars.next();
            } else {
                break;
            }
        }
    }

    fn is_name_start(ch: char) -> bool {
        ch.is_alphabetic() || ch == '_'
    }

    fn is_name_continue(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    fn is_whitespace(ch: char) -> bool {
        ch.is_ascii_whitespace()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_empty() {
        let doc = String::from("");
        let mut tokenizer = Tokenizer::default();
        let actual = tokenizer.tokenize(doc.to_string()).unwrap();
        let expected = vec![Token::Eof];
        assert_eq!(expected, actual);
    }

    #[test]
    fn tokenizer_errors_know_position() {
        let doc = String::from("        \n  1asd");
        let mut tokenizer = Tokenizer::default();
        let actual = tokenizer.tokenize(doc.to_string());
        let expected = Err(TokenizerError::new(
            "unrecognized character: `1`".to_string(),
            2,
            3,
        ));
        assert_eq!(expected, actual);
    }

    #[test]
    fn tokenize_something() {
        let doc = String::from("type Person { name: String }");
        let mut tokenizer = Tokenizer::default();
        let actual = tokenizer.tokenize(doc.to_string()).unwrap();
        let expected = vec![
            Token::keyword("type"),
            Token::name(String::from("Person")),
            Token::punctuator('{'),
            Token::name(String::from("name")),
            Token::punctuator(':'),
            Token::name(String::from("String")),
            Token::punctuator('}'),
            Token::Eof,
        ];
        assert_eq!(expected, actual);
    }

    #[test]
    fn tokenize_string_value() {
        let doc = String::from("\"The quick brown fox jumps over the lazy dog\"");
        let mut tokenizer = Tokenizer::default();
        let actual = tokenizer.tokenize(doc.to_string()).unwrap();
        let expected = vec![
            Token::string_value(String::from("The quick brown fox jumps over the lazy dog")),
            Token::Eof,
        ];
        assert_eq!(expected, actual);
    }
}

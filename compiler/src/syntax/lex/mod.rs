use std::{
    io::{Error, ErrorKind},
    str::FromStr,
};

use self::{
    cursor::{Cursor, END_OF_FILE},
    token::{AccessType, Numeric, StringType, Token, TokenType},
};

use crate::token;

use super::ast::keyword::{Keyword, MAX_KEYWORD_LENGTH};

pub(crate) mod cursor;
pub mod token;

/// A trait that may be used to implement future implementations of PHP.
pub(crate) trait Tokenizer<'a> {
    fn lex(&mut self, cursor: &'a mut Cursor) -> Result<Token, Error>;
}

/// The basic PHP Lexer, Serves the syntax of PHP 7.3+
impl Cursor<'_> {
    fn eat(&mut self) -> Result<Option<Token>, Error> {
        let start_pos = self.get_pos();

        if let Some(spaces) = self.eat_whitespace()? {
            return token!(
                start_pos,
                self.get_pos(),
                TokenType::Whitespace,
                Some(spaces)
            );
        }

        if let Some(comment) = self.eat_comment()? {
            return token!(start_pos, self.get_pos(), TokenType::Comment, Some(comment));
        }

        if let Some(operator) = self.eat_operator()? {
            return token!(
                start_pos,
                self.get_pos(),
                TokenType::Operator,
                Some(operator)
            );
        }

        if let Some(keyword) = self.eat_keyword()? {
            return token!(start_pos, self.get_pos(), TokenType::Keyword(keyword), None);
        }

        if let Some(boolean) = self.eat_boolean()? {
            return token!(start_pos, self.get_pos(), TokenType::Boolean, Some(boolean));
        }

        if let Some(identifier) = self.eat_identifier()? {
            return token!(
                start_pos,
                self.get_pos(),
                TokenType::Identifier,
                Some(identifier)
            );
        }

        if let Some(n) = self.eat_number()? {
            return token!(start_pos, self.get_pos(), TokenType::NumericalLit(n));
        }

        if let Some((var, string)) = self.eat_string()? {
            self.peek(); // what?
            return token!(
                start_pos,
                self.get_pos(),
                TokenType::StringLit(var),
                Some(string)
            );
        }

        if let Some(token_type) = self.eat_value_reserved()? {
            return token!(start_pos, self.get_pos(), token_type.0, Some(token_type.1));
        }

        if let Some(token_type) = self.eat_reserved()? {
            // Peek if a reserved character is found
            self.peek();
            return token!(start_pos, self.get_pos(), token_type);
        }

        self.peek();
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "Failed to parse a token from buffer: {} until {}",
                start_pos,
                self.get_pos()
            ),
        ));
    }

    fn eat_comment(&mut self) -> Result<Option<String>, Error> {
        return Ok(match self.first()? {
            '/' => {
                // check the next character
                if self.second()? == '/' {
                    Some(self.eat_while(|c| c != '\n')?)
                } else if self.second()? == '*' {
                    // eat the comment
                    let comment = self.eat_while_cursor(|cursor, c| {
                        if c == '*' {
                            if cursor.first().unwrap() == '/' {
                                cursor.eat();
                                return false;
                            } else {
                                return true;
                            }
                        } else {
                            return true;
                        }
                    })?;
                    Some(comment)
                } else {
                    None
                }
            }
            _ => None,
        });
    }

    /// This may be misleading,
    /// because it eats ALL whitespace until a char is not whitespace
    fn eat_whitespace(&mut self) -> Result<Option<String>, Error> {
        let segment = self.eat_while(|c| c.is_whitespace())?;
        return if segment.is_empty() {
            Ok(None)
        } else {
            Ok(Some(segment))
        };
    }

    fn eat_identifier(&mut self) -> Result<Option<String>, Error> {
        Ok(match self.first()? {
            // 'A'..='z' can't be used here as it includes a plethora of reserved characters that are used elsewhere
            '_' | 'a'..='z' | 'A'..='Z' => Some(
                self.eat_while(|c: char| !c.is_whitespace() && (c.is_alphanumeric() || c == '_'))?,
            ),
            _ => None,
        })
    }

    fn eat_number(&mut self) -> Result<Option<Numeric>, Error> {
        Ok(match self.first()? {
            // there is an issue with leading floats where they are parsed as accessors right now.
            // we should leave this to the parser.
            '0'..='9' => {
                // do this in the background,
                // todo ACTUALLY IMPLEMENT THIS
                self.eat_while(|c: char| c.is_digit(10) || c == '.');
                Some(Numeric::Int(0))
            }
            _ => None,
        })
    }

    /// Eats a keyword but does not parse it.
    fn eat_keyword(&mut self) -> Result<Option<Keyword>, Error> {
        let mut segment = String::new();
        for i in 0..MAX_KEYWORD_LENGTH {
            segment.push(self.nth_char(i)?);

            if let Ok(keyword) = Keyword::from_str(&segment) {
                if self.nth_char(i + 1)?.is_whitespace() {
                    self.peek_inc(i);
                    return Ok(Some(keyword));
                } else {
                    return Ok(None);
                }
            }
        }

        return Ok(None);
    }

    fn eat_operator(&mut self) -> Result<Option<String>, Error> {
        Ok(match self.first()? {
            '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '&' | '|' | '^' | '~' => {
                self.peek();
                Some(self.get_prev().to_string())
            }
            'o' => {
                if self.nth_char(1)? == 'r' {
                    self.peek_inc(2);
                    Some("or".to_string())
                } else {
                    None
                }
            }
            'a' => {
                if self.nth_char(1)? == 'n' && self.nth_char(2)? == 'd' {
                    self.peek_inc(3);
                    Some("and".to_string())
                } else {
                    None
                }
            }
            _ => None,
        })
    }

    fn eat_boolean(&mut self) -> Result<Option<String>, Error> {
        // there is probably a better way to do this.
        let mut segment = String::new();
        for i in 0..4 {
            segment.push(self.nth_char(i)?);

            if segment == "true" || segment == "false" {
                self.peek_inc(i);
                return Ok(Some(segment));
            }
        }
        return Ok(None);
    }

    fn eat_string(&mut self) -> Result<Option<(StringType, String)>, Error> {
        if self.first()? != '"' && self.first()? != '\'' && self.first()? != '`' {
            return Ok(None);
        } else {
            let first = self.peek().unwrap();
            let variant = match first {
                '"' => StringType::Double,
                '\'' => StringType::Single,
                _ => unreachable!(),
            };
            return Ok(Some((variant, self.eat_while(|c| c != first)?)));
        }
    }

    fn eat_value_reserved(&mut self) -> Result<Option<(TokenType, String)>, Error> {
        Ok(match self.first()? {
            ':' => {
                if self.second()? == ':' {
                    self.peek_inc(1);
                    Some((
                        TokenType::Accessor(AccessType::StaticMember),
                        "::".to_string(),
                    ))
                } else {
                    self.peek();
                    Some((TokenType::Colon, ":".to_string()))
                }
            }
            _ => None,
        })
    }

    fn eat_reserved(&mut self) -> Result<Option<TokenType>, Error> {
        Ok(match self.first()? {
            '[' => Some(TokenType::LeftBracket),
            ']' => Some(TokenType::RightBracket),
            '(' => Some(TokenType::LeftParenthesis),
            ')' => Some(TokenType::RightParenthesis),
            '{' => Some(TokenType::LeftBrace),
            '}' => Some(TokenType::RightBrace),
            ';' => Some(TokenType::EOS),
            ',' => Some(TokenType::Comma),
            '\\' => Some(TokenType::Backslash),
            '.' => Some(TokenType::Dot),
            '$' => Some(TokenType::Variable),
            '?' => Some(TokenType::QuestionMark),
            _ => None,
        })
    }
}

pub struct Lexer<'a> {
    cursor: Cursor<'a>,
}

impl<'a> Lexer<'a> {
    pub fn new(script: &'a str) -> Self {
        Self {
            cursor: Cursor::new(script),
        }
    }
    /// Consumes the next possible token(s).
    pub fn next(&mut self) -> Result<Option<Token>, Error> {
        if let Some(v) = self.cursor.eat()? {
            return Ok(Some(v));
        } else {
            return Ok(None);
        }
    }
}

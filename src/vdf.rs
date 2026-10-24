//! Small bounded KeyValues reader for Steam's library and app manifests.
use anyhow::{Result, bail, ensure};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(pairs) => pairs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn text(&self) -> Option<&str> {
        if let Self::Text(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn pairs(&self) -> &[(String, Value)] {
        if let Self::Object(v) = self { v } else { &[] }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Text(String),
    Open,
    Close,
}

pub fn parse(input: &str) -> Result<Value> {
    ensure!(
        input.len() <= 4 * 1024 * 1024,
        "Steam manifest exceeds 4 MiB"
    );
    let mut chars = input.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(ch) = chars.next() {
        match ch {
            c if c.is_whitespace() || c == '\u{feff}' => (),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '"' => {
                let mut text = String::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == '"' {
                        closed = true;
                        break;
                    }
                    if c == '\\' {
                        let next = chars
                            .next()
                            .ok_or_else(|| anyhow::anyhow!("Unterminated VDF escape"))?;
                        match next {
                            '\\' | '"' => text.push(next),
                            _ => {
                                text.push('\\');
                                text.push(next);
                            }
                        }
                    } else {
                        text.push(c);
                    }
                }
                ensure!(closed, "Unterminated VDF string");
                tokens.push(Token::Text(text));
            }
            c => {
                let mut text = String::from(c);
                while chars
                    .peek()
                    .is_some_and(|c| !c.is_whitespace() && *c != '{' && *c != '}')
                {
                    if let Some(c) = chars.next() {
                        text.push(c);
                    }
                }
                tokens.push(Token::Text(text));
            }
        }
    }
    fn object(tokens: &[Token], i: &mut usize, nested: bool, depth: usize) -> Result<Value> {
        ensure!(depth <= 32, "VDF nesting exceeds 32 levels");
        let mut pairs = Vec::new();
        while *i < tokens.len() {
            if tokens[*i] == Token::Close {
                ensure!(nested, "Unexpected closing VDF brace");
                *i += 1;
                return Ok(Value::Object(pairs));
            }
            let Token::Text(key) = &tokens[*i] else {
                bail!("Expected VDF key");
            };
            let key = key.clone();
            *i += 1;
            let value = match tokens.get(*i) {
                Some(Token::Text(value)) => {
                    *i += 1;
                    Value::Text(value.clone())
                }
                Some(Token::Open) => {
                    *i += 1;
                    object(tokens, i, true, depth + 1)?
                }
                _ => bail!("Missing VDF value for {key}"),
            };
            pairs.push((key, value));
        }
        ensure!(!nested, "Missing closing VDF brace");
        Ok(Value::Object(pairs))
    }
    object(&tokens, &mut 0, false, 0)
}

pub fn library_paths(text: &str) -> Result<Vec<String>> {
    let root = parse(text)?;
    let root = root.get("libraryfolders").unwrap_or(&root);
    Ok(root
        .pairs()
        .iter()
        .filter(|(key, _)| key.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|(_, value)| {
            value
                .text()
                .or_else(|| value.get("path").and_then(Value::text))
        })
        .map(str::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_steam_formats_and_escaped_paths() {
        let text = r#"// libraries
        "libraryfolders" { "0" "/mnt/old library" "1" { "path" "C:\\Steam" "apps" { "431960" "42" } } }"#;
        assert_eq!(
            library_paths(text).expect("parse"),
            vec!["/mnt/old library", "C:\\Steam"]
        );
    }
    #[test]
    fn rejects_truncated_and_deep_manifests() {
        assert!(parse(r#""libraryfolders" { "0""#).is_err());
        assert!(parse(&format!("{}{}", "a {".repeat(40), "}".repeat(40))).is_err());
    }
}

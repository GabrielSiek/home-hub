//! Minimal reader for Valve's KeyValues text format (.vdf / .acf files).

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Str(String),
    Obj(Vec<(String, Vdf)>),
}

impl Vdf {
    /// Child by key, ignoring case: Steam isn't consistent about it ("apps" vs "Apps").
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        self.entries()
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn path(&self, keys: &[&str]) -> Option<&Vdf> {
        keys.iter().try_fold(self, |v, k| v.get(k))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Obj(_) => None,
        }
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Obj(items) => items,
            Vdf::Str(_) => &[],
        }
    }
}

/// Parses a whole file; its top-level pairs come back as one object. Lenient: malformed
/// input yields whatever was read before the problem.
pub fn parse(src: &str) -> Vdf {
    let mut t = Tokens {
        chars: src.trim_start_matches('\u{feff}').chars().collect(),
        pos: 0,
    };
    Vdf::Obj(parse_obj(&mut t))
}

fn parse_obj(t: &mut Tokens) -> Vec<(String, Vdf)> {
    let mut items = Vec::new();
    loop {
        match t.next() {
            None | Some(Tok::Close) => break,
            Some(Tok::Open) => {
                parse_obj(t); // a block without a key: skip it
            }
            Some(Tok::Str(key)) => match t.next() {
                Some(Tok::Str(v)) => items.push((key, Vdf::Str(v))),
                Some(Tok::Open) => items.push((key, Vdf::Obj(parse_obj(t)))),
                _ => break,
            },
        }
    }
    items
}

enum Tok {
    Str(String),
    Open,
    Close,
}

struct Tokens {
    chars: Vec<char>,
    pos: usize,
}

impl Tokens {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.pos + ahead).copied()
    }

    fn next(&mut self) -> Option<Tok> {
        loop {
            let c = self.peek(0)?;
            if c.is_whitespace() {
                self.pos += 1;
            } else if c == '/' && self.peek(1) == Some('/') {
                while self.peek(0).is_some_and(|c| c != '\n') {
                    self.pos += 1;
                }
            } else if c == '{' {
                self.pos += 1;
                return Some(Tok::Open);
            } else if c == '}' {
                self.pos += 1;
                return Some(Tok::Close);
            } else if c == '"' {
                self.pos += 1;
                return Some(Tok::Str(self.quoted()));
            } else {
                let word = self.unquoted();
                // Platform conditionals like [$WIN32] follow a value; they aren't keys.
                if !(word.starts_with('[') && word.ends_with(']')) {
                    return Some(Tok::Str(word));
                }
            }
        }
    }

    fn quoted(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek(0) {
            self.pos += 1;
            match c {
                '"' => break,
                '\\' => {
                    let e = self.peek(0).unwrap_or('\\');
                    self.pos += 1;
                    s.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                }
                _ => s.push(c),
            }
        }
        s
    }

    fn unquoted(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek(0) {
            if c.is_whitespace() || matches!(c, '{' | '}' | '"') {
                break;
            }
            s.push(c);
            self.pos += 1;
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_objects_and_case_insensitive_lookup() {
        let doc = parse(
            "\u{feff}\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"228980\"\t\t\"0\"\n\t\t}\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\Games\"\n\t}\n}\n",
        );
        let lf = doc.get("LibraryFolders").unwrap();
        assert_eq!(lf.entries().len(), 2);
        assert_eq!(lf.path(&["0", "path"]).unwrap().as_str(), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(lf.path(&["1", "path"]).unwrap().as_str(), Some(r"D:\Games"));
        assert_eq!(lf.path(&["0", "apps", "228980"]).unwrap().as_str(), Some("0"));
    }

    #[test]
    fn comments_escapes_and_conditionals() {
        let doc = parse(
            "// header\n\"a\" { \"quote\" \"say \\\"hi\\\"\" [$WIN32] \"bare\" value }\n\"b\" \"2\"",
        );
        assert_eq!(doc.path(&["a", "quote"]).unwrap().as_str(), Some("say \"hi\""));
        assert_eq!(doc.path(&["a", "bare"]).unwrap().as_str(), Some("value"));
        assert_eq!(doc.get_str("b"), Some("2"));
    }

    #[test]
    fn truncated_input_keeps_what_was_read() {
        let doc = parse("\"a\" { \"x\" \"1\" \"y\"");
        assert_eq!(doc.path(&["a", "x"]).unwrap().as_str(), Some("1"));
    }
}

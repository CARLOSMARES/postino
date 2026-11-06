//! Converting Postman `pm.*` scripts to Postino's script API (`docs/scripting.md`).
//!
//! The source is split into top level statements, and every known `pm.*` call in a statement is
//! rewritten. A statement that still uses something Postino does not have (`pm.sendRequest`,
//! `CryptoJS`, ...) is kept as a comment, so a script never fails on an API that does not exist.
//! A script that cannot even be split, because its brackets do not balance as this reads them, is
//! commented out whole.
//!
//! This works on tokens outside strings and comments, without a JavaScript parser. It only has to
//! be safe: what it does not understand ends up commented out, never silently changed.

/// The first line of a script with commented out statements.
pub(super) const PARTIAL_HEADER: &str =
    "// Imported from Postman. What could not be converted is commented out below.";

/// The result of [`convert_script`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConvertedScript {
    /// The script to write.
    pub(super) text: String,
    /// How many statements it has, comments and blank lines left out.
    pub(super) total: usize,
    /// How many of them were kept as code.
    pub(super) converted: usize,
    /// Whether anything used `pm.sendRequest`, the most common reason for a comment.
    pub(super) send_request: bool,
    /// Whether anything was rewritten.
    pub(super) rewrote: bool,
}

/// Rewrites without an argument: the text after `pm.` to its replacement. Longest first where
/// one is the prefix of another.
const REWRITES: &[(&str, &str)] = &[
    ("pm.test(", "test("),
    (
        "pm.response.to.be.ok",
        "expect(res.status).toBeLessThan(300)",
    ),
    (
        "pm.response.to.be.success",
        "expect(res.status).toBeLessThan(300)",
    ),
    ("pm.response.code", "res.status"),
    ("pm.response.json()", "res.json()"),
    ("pm.response.text()", "res.body"),
    ("pm.response.headers.get(", "res.headers.get("),
    ("pm.response.responseTime", "res.timeMs"),
    ("pm.environment.set(", "env.set("),
    ("pm.environment.get(", "env.get("),
    ("pm.environment.unset(", "env.unset("),
    ("pm.collectionVariables.set(", "env.set("),
    ("pm.collectionVariables.get(", "env.get("),
    ("pm.collectionVariables.unset(", "env.unset("),
    ("pm.globals.set(", "env.set("),
    ("pm.globals.get(", "env.get("),
    ("pm.globals.unset(", "env.unset("),
    ("pm.variables.set(", "vars.set("),
    ("pm.request.url", "req.url"),
    ("pm.request.method", "req.method"),
    ("postman.setEnvironmentVariable(", "env.set("),
    ("postman.getEnvironmentVariable(", "env.get("),
    ("postman.clearEnvironmentVariable(", "env.unset("),
];

/// What is left of Postman's API after the rewrites, each making its statement a comment.
const UNSUPPORTED: &[&str] = &[
    "pm",
    "postman",
    "CryptoJS",
    "require",
    "responseBody",
    "responseCode",
    "responseTime",
    "responseHeaders",
    "tests",
    "xml2Json",
    "cheerio",
    "tv4",
    "atob",
    "btoa",
];

/// Converts a script. See the module docs.
pub(super) fn convert_script(source: &str) -> ConvertedScript {
    let Some(segments) = split_statements(source) else {
        let code_lines = source
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        return ConvertedScript {
            text: comment_out(source),
            total: code_lines,
            converted: 0,
            send_request: source.contains("pm.sendRequest"),
            rewrote: false,
        };
    };
    let mut text = String::new();
    let (mut total, mut converted) = (0, 0);
    let (mut send_request, mut rewrote) = (false, false);
    for segment in segments {
        if !has_code(segment) {
            text.push_str(segment);
            continue;
        }
        total += 1;
        let rewritten = rewrite(segment);
        if uses_unsupported(&rewritten) {
            send_request |= segment.contains("pm.sendRequest");
            text.push_str(&comment_out(segment));
            if segment.ends_with('\n') {
                text.push('\n');
            }
        } else {
            converted += 1;
            rewrote |= rewritten != segment;
            text.push_str(&rewritten);
        }
    }
    let text = text.trim_end().to_string();
    let text = if converted < total {
        format!("{PARTIAL_HEADER}\n{text}")
    } else {
        text
    };
    ConvertedScript {
        text,
        total,
        converted,
        send_request,
        rewrote,
    }
}

/// `source` with `// ` in front of every line.
fn comment_out(source: &str) -> String {
    source
        .trim_end_matches('\n')
        .lines()
        .map(|line| format!("// {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A position of a script as a scanner sees it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lexeme {
    /// Code: this byte starts a character outside strings and comments.
    Code,
    /// Inside a string or a comment.
    Skipped,
}

/// Classifies every byte of `source`.
fn scan(source: &str) -> Vec<Lexeme> {
    let bytes = source.as_bytes();
    let mut map = vec![Lexeme::Code; bytes.len()];
    let mut index = 0;
    while index < bytes.len() {
        let end = match bytes[index] {
            quote @ (b'"' | b'\'' | b'`') => {
                let mut end = index + 1;
                while end < bytes.len() && bytes[end] != quote {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                (end + 1).min(bytes.len())
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => source[index..]
                .find('\n')
                .map_or(bytes.len(), |offset| index + offset),
            b'/' if bytes.get(index + 1) == Some(&b'*') => source[index + 2..]
                .find("*/")
                .map_or(bytes.len(), |offset| index + 2 + offset + 2),
            _ => {
                index += 1;
                continue;
            }
        };
        map[index..end].fill(Lexeme::Skipped);
        index = end;
    }
    map
}

/// Whether `segment` has anything but blanks, comments and semicolons.
fn has_code(segment: &str) -> bool {
    let map = scan(segment);
    segment.char_indices().any(|(index, character)| {
        map[index] == Lexeme::Code && !character.is_whitespace() && character != ';'
    })
}

/// Splits `source` into top level statements, with blanks and comments as segments of their own
/// or attached to a statement. Concatenating them gives `source` back. `None` when the brackets
/// do not balance.
fn split_statements(source: &str) -> Option<Vec<&str>> {
    let map = scan(source);
    let mut segments = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (index, character) in source.char_indices() {
        if map[index] != Lexeme::Code {
            // A line comment ends at its newline, which the scanner leaves as code.
            continue;
        }
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            ';' if depth == 0 => {
                let end = source[index + 1..]
                    .find(|c: char| c == '\n' || !c.is_whitespace())
                    .map_or(source.len(), |offset| {
                        let at = index + 1 + offset;
                        if source[at..].starts_with('\n') {
                            at + 1
                        } else {
                            index + 1
                        }
                    });
                if end > start {
                    segments.push(&source[start..end]);
                    start = end;
                }
            }
            '\n' if depth == 0
                && index >= start
                && !continues(&source[start..index], &source[index + 1..]) =>
            {
                segments.push(&source[start..=index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    if start < source.len() {
        segments.push(&source[start..]);
    }
    Some(segments)
}

/// Whether a statement whose text so far is `before` goes on after the newline, in `after`.
fn continues(before: &str, after: &str) -> bool {
    if before.trim().is_empty() {
        return false;
    }
    const TRAILING: &[&str] = &[
        "=", "+", "-", "*", "/", ",", "?", ":", ".", "&&", "||", "=>", "!",
    ];
    let before = before.trim_end();
    if TRAILING.iter().any(|end| before.ends_with(end)) && !last_line_is_comment(before) {
        return true;
    }
    let next = after.trim_start();
    if next.starts_with("//") || next.starts_with("/*") {
        return false;
    }
    const LEADING: &[&str] = &[".", "?", ":", "&&", "||", "+", "-", "*", "/", "=", ")"];
    LEADING.iter().any(|start| next.starts_with(start))
        || ["else", "catch", "finally"]
            .iter()
            .any(|word| starts_with_word(next, word))
}

fn last_line_is_comment(text: &str) -> bool {
    text.lines()
        .last()
        .is_some_and(|line| line.trim_start().starts_with("//"))
}

fn starts_with_word(text: &str, word: &str) -> bool {
    text.strip_prefix(word)
        .is_some_and(|rest| !rest.starts_with(is_identifier_char))
}

fn is_identifier_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

/// Whether the code at `index` starts a new identifier: not in the middle of one, nor a property
/// of something else.
fn at_word_start(code: &str, index: usize) -> bool {
    !code[..index]
        .chars()
        .next_back()
        .is_some_and(|before| is_identifier_char(before) || before == '.')
}

/// Whether the rewritten `code` still names a part of Postman's API.
fn uses_unsupported(code: &str) -> bool {
    let map = scan(code);
    code.char_indices().any(|(index, _)| {
        map[index] == Lexeme::Code
            && at_word_start(code, index)
            && UNSUPPORTED
                .iter()
                .any(|name| starts_with_word(&code[index..], name))
    })
}

/// Rewrites every known `pm.*` use in `code`.
fn rewrite(code: &str) -> String {
    let map = scan(code);
    let mut out = String::with_capacity(code.len());
    let mut index = 0;
    while index < code.len() {
        let rest = &code[index..];
        if map[index] == Lexeme::Code
            && at_word_start(code, index)
            && let Some((text, used)) = rewrite_at(rest)
        {
            out.push_str(&text);
            index += used;
            continue;
        }
        let character = rest.chars().next().unwrap_or_default();
        out.push(character);
        index += character.len_utf8().max(1);
    }
    out
}

/// The rewrite of the `pm.*` use `rest` starts with: the new text and how much of `rest` it
/// replaces.
fn rewrite_at(rest: &str) -> Option<(String, usize)> {
    if rest.starts_with("pm.expect(") {
        return rewrite_expect(rest);
    }
    if let Some(after) = rest.strip_prefix("pm.response.to.have.status(") {
        let (argument, used) = balanced(after)?;
        let text = format!("expect(res.status).toBe({})", rewrite(argument));
        return Some((text, rest.len() - after.len() + used));
    }
    if let Some(after) = rest.strip_prefix("pm.variables.get(") {
        let (argument, used) = balanced(after)?;
        let argument = rewrite(argument);
        let text = format!("(vars.get({argument}) ?? env.get({argument}))");
        return Some((text, rest.len() - after.len() + used));
    }
    if let Some(after) = rest.strip_prefix("pm.request.headers.add(") {
        let (argument, used) = balanced(after)?;
        let (key, value) = header_object(argument)?;
        let text = format!("req.headers.set({}, {})", rewrite(key), rewrite(value));
        return Some((text, rest.len() - after.len() + used));
    }
    REWRITES.iter().find_map(|(from, to)| {
        let after = rest.strip_prefix(from)?;
        // `pm.response.code` must not match `pm.response.codes`.
        let ends_word = from.ends_with(['(', ')']) || !after.starts_with(is_identifier_char);
        ends_word.then(|| (to.to_string(), from.len()))
    })
}

/// `key` and `value` of a `{ key: ..., value: ... }` object, the form `pm.request.headers.add`
/// is called with.
fn header_object(argument: &str) -> Option<(&str, &str)> {
    let inner = argument.trim().strip_prefix('{')?.strip_suffix('}')?;
    let (first, second) = split_top_level_comma(inner)?;
    let value_of = |text: &str, name: &str| -> Option<(usize, usize)> {
        let colon = text.find(':')?;
        let label = text[..colon].trim().trim_matches(['"', '\'']);
        (label == name).then_some((colon + 1, text.len()))
    };
    let pick = |name: &str| {
        [first, second].into_iter().find_map(|part| {
            let (start, end) = value_of(part, name)?;
            Some(part[start..end].trim())
        })
    };
    Some((pick("key")?, pick("value")?))
}

/// Splits `text` at its only top level comma, ignoring one that ends it.
fn split_top_level_comma(text: &str) -> Option<(&str, &str)> {
    let map = scan(text);
    let mut depth = 0;
    for (index, character) in text.char_indices() {
        if map[index] != Lexeme::Code {
            continue;
        }
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                let second = text[index + 1..].trim().trim_end_matches(',');
                return Some((&text[..index], second));
            }
            _ => {}
        }
    }
    None
}

/// The text up to the `)` that closes an already open parenthesis, and how far past it `text` was
/// read. `None` when it never closes.
fn balanced(text: &str) -> Option<(&str, usize)> {
    let map = scan(text);
    let mut depth = 1;
    for (index, character) in text.char_indices() {
        if map[index] != Lexeme::Code {
            continue;
        }
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&text[..index], index + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// Rewrites `pm.expect(value).to...`. Returns `None` for a chain Postino has no matcher for, so
/// the statement ends up as a comment.
fn rewrite_expect(rest: &str) -> Option<(String, usize)> {
    let after_open = rest.strip_prefix("pm.expect(")?;
    let (value, used) = balanced(after_open)?;
    let open_len = rest.len() - after_open.len();
    let chain_start = open_len + used;
    let (words, chain_end) = read_chain(&rest[chain_start..]);
    let mut negated = false;
    let mut deep = false;
    let mut terminal = None;
    for word in &words {
        match *word {
            "to" | "be" | "been" | "is" | "that" | "which" | "and" | "has" | "have" | "with"
            | "at" | "of" | "same" => {}
            "not" => negated = !negated,
            "deep" => deep = true,
            other => {
                if terminal.is_some() {
                    return None;
                }
                terminal = Some(other);
            }
        }
    }
    let after_chain = &rest[chain_start + chain_end..];
    let matcher = match terminal? {
        "equal" | "equals" | "eq" => {
            if deep {
                "toEqual"
            } else {
                "toBe"
            }
        }
        "eql" | "eqls" => "toEqual",
        "include" | "includes" | "contain" | "contains" => "toContain",
        "property" => "toHaveProperty",
        "above" | "gt" | "greaterThan" => "toBeGreaterThan",
        "below" | "lt" | "lessThan" => "toBeLessThan",
        "match" | "matches" => "toMatch",
        constant @ ("true" | "false" | "null" | "undefined") => {
            let not = if negated { ".not" } else { "" };
            let text = format!("expect({}){not}.toBe({constant})", rewrite(value));
            return Some((text, chain_start + chain_end));
        }
        "ok" => {
            let not = if negated { ".not" } else { "" };
            let text = format!("expect({}){not}.toBeTruthy()", rewrite(value));
            return Some((text, chain_start + chain_end));
        }
        "exist" => {
            let not = if negated { "" } else { ".not" };
            let text = format!("expect({}){not}.toBe(undefined)", rewrite(value));
            return Some((text, chain_start + chain_end));
        }
        _ => return None,
    };
    let after_paren = after_chain.strip_prefix('(')?;
    let not = if negated { ".not" } else { "" };
    let text = format!("expect({}){not}.{matcher}(", rewrite(value));
    Some((text, rest.len() - after_paren.len()))
}

/// The `.word.word` chain `text` starts with, line breaks allowed before a dot, and its length.
fn read_chain(text: &str) -> (Vec<&str>, usize) {
    let mut words = Vec::new();
    let mut index = 0;
    loop {
        let next = &text[index..];
        let trimmed = next.trim_start();
        let Some(after_dot) = trimmed.strip_prefix('.') else {
            return (words, index);
        };
        let length = after_dot
            .find(|character: char| !is_identifier_char(character))
            .unwrap_or(after_dot.len());
        if length == 0 {
            return (words, index);
        }
        words.push(&after_dot[..length]);
        index += next.len() - trimmed.len() + 1 + length;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn converted(source: &str) -> String {
        let result = convert_script(source);
        assert_eq!(result.converted, result.total, "{}", result.text);
        result.text
    }

    #[test]
    fn the_documented_table_converts() {
        for (from, to) in [
            ("pm.test(\"a\", () => {});", "test(\"a\", () => {});"),
            ("pm.expect(a).to.equal(b);", "expect(a).toBe(b);"),
            ("pm.expect(a).to.eql(b);", "expect(a).toEqual(b);"),
            ("pm.expect(a).to.deep.equal(b);", "expect(a).toEqual(b);"),
            ("pm.expect(a).to.include(b);", "expect(a).toContain(b);"),
            (
                "pm.expect(a).to.have.property(\"p\");",
                "expect(a).toHaveProperty(\"p\");",
            ),
            (
                "pm.expect(a).to.be.above(1);",
                "expect(a).toBeGreaterThan(1);",
            ),
            ("pm.expect(a).to.be.below(1);", "expect(a).toBeLessThan(1);"),
            ("pm.expect(a).to.not.equal(b);", "expect(a).not.toBe(b);"),
            ("pm.expect(a).to.be.true;", "expect(a).toBe(true);"),
            ("pm.expect(a).to.exist;", "expect(a).not.toBe(undefined);"),
            (
                "pm.response.to.have.status(200);",
                "expect(res.status).toBe(200);",
            ),
            (
                "pm.response.to.be.ok;",
                "expect(res.status).toBeLessThan(300);",
            ),
            ("const c = pm.response.code;", "const c = res.status;"),
            ("const j = pm.response.json();", "const j = res.json();"),
            ("const t = pm.response.text();", "const t = res.body;"),
            ("pm.response.headers.get(\"x\");", "res.headers.get(\"x\");"),
            (
                "const t = pm.response.responseTime;",
                "const t = res.timeMs;",
            ),
            ("pm.environment.set(\"k\", 1);", "env.set(\"k\", 1);"),
            (
                "pm.collectionVariables.set(\"k\", 1);",
                "env.set(\"k\", 1);",
            ),
            ("pm.globals.set(\"k\", 1);", "env.set(\"k\", 1);"),
            ("pm.environment.get(\"k\");", "env.get(\"k\");"),
            ("pm.environment.unset(\"k\");", "env.unset(\"k\");"),
            ("pm.variables.set(\"k\", 1);", "vars.set(\"k\", 1);"),
            (
                "pm.variables.get(\"k\");",
                "(vars.get(\"k\") ?? env.get(\"k\"));",
            ),
            (
                "pm.request.headers.add({ key: \"a\", value: b });",
                "req.headers.set(\"a\", b);",
            ),
            ("const u = pm.request.url;", "const u = req.url;"),
            (
                "postman.setEnvironmentVariable(\"k\", 1);",
                "env.set(\"k\", 1);",
            ),
        ] {
            assert_eq!(converted(from), to);
        }
    }

    #[test]
    fn a_multiline_test_converts_inside_and_out() {
        let source = "pm.test('status', function () {\n  pm.response.to.have.status(200);\n  const body = pm.response.json();\n  pm.expect(body.id)\n    .to.equal(pm.environment.get('id'));\n});";
        assert_eq!(
            converted(source),
            "test('status', function () {\n  expect(res.status).toBe(200);\n  const body = res.json();\n  expect(body.id).toBe(env.get('id'));\n});"
        );
    }

    #[test]
    fn strings_and_comments_are_left_alone() {
        let source = "// pm.test is what this was\nconst a = \"pm.response.code\";\nconst b = `pm.globals.set(1)`;";
        assert_eq!(converted(source), source);
    }

    #[test]
    fn an_unknown_api_comments_out_its_statement_only() {
        let source = "pm.environment.set('a', 1);\npm.sendRequest('http://x', (err, res) => {\n  console.log(res);\n});\nconst c = pm.response.code;";
        let result = convert_script(source);
        assert_eq!(result.total, 3);
        assert_eq!(result.converted, 2);
        assert!(result.send_request);
        assert_eq!(
            result.text,
            format!(
                "{PARTIAL_HEADER}\nenv.set('a', 1);\n// pm.sendRequest('http://x', (err, res) => {{\n//   console.log(res);\n// }});\nconst c = res.status;"
            )
        );
    }

    #[test]
    fn an_unsupported_matcher_comments_out_the_test_that_uses_it() {
        let source = "pm.test('t', () => {\n  pm.expect(a).to.be.an('array');\n});";
        let result = convert_script(source);
        assert_eq!((result.total, result.converted), (1, 0));
        assert!(result.text.contains("// pm.test('t', () => {"));
    }

    #[test]
    fn other_names_are_not_confused_with_postman() {
        let source = "const pmo = 1;\nfoo.pm.bar();\nconst tests2 = 3;";
        assert_eq!(converted(source), source);
    }

    #[test]
    fn crypto_and_require_are_not_available() {
        for source in [
            "const h = CryptoJS.MD5('a');",
            "const m = require('moment');",
            "tests['ok'] = true;",
            "const b = responseBody;",
        ] {
            let result = convert_script(source);
            assert_eq!(result.converted, 0, "{source}");
            assert!(result.text.contains(&format!("// {source}")), "{source}");
        }
    }

    #[test]
    fn unbalanced_scripts_are_commented_out_whole() {
        let source = "pm.test('t', () => {\n  a(;\n";
        let result = convert_script(source);
        assert_eq!(result.converted, 0);
        assert!(result.text.lines().all(|line| line.starts_with("//")));
    }

    #[test]
    fn scripts_without_postman_stay_as_they_are() {
        let source = "const a = 1;\nif (a) {\n  console.log(a);\n} else {\n  console.log(0);\n}";
        let result = convert_script(source);
        assert_eq!(result.text, source);
        assert!(!result.rewrote);
    }
}

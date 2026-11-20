use crate::lexer::{self, SourceId, Span};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub text: String,
}

/// Maps `SourceId`s to files so diagnostics can be rendered for any span.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn add(&mut self, path: impl Into<PathBuf>, text: String) -> SourceId {
        let id = SourceId(self.files.len());
        self.files.push(SourceFile {
            path: path.into(),
            text,
        });
        id
    }

    pub fn get(&self, id: SourceId) -> Option<&SourceFile> {
        self.files.get(id.0)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Render a span-qualified diagnostic.
    pub fn format(&self, span: Span, message: &str) -> String {
        match self.get(span.source) {
            Some(file) => {
                let (line, column) = lexer::line_col(&file.text, span.start);
                format!("{}:{line}:{column}: error: {message}", file.path.display())
            }
            None => format!("error: {message}"),
        }
    }

    pub fn format_lex_errors(&self, errors: &[lexer::Error]) -> String {
        errors
            .iter()
            .map(|error| match self.get(error.source) {
                Some(file) => {
                    let (line, column) = lexer::line_col(&file.text, error.offset);
                    format!(
                        "{}:{line}:{column}: error: {}",
                        file.path.display(),
                        error.message
                    )
                }
                None => format!("error: {}", error.message),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_a_span_with_its_file() {
        let mut map = SourceMap::default();
        let id = map.add("src/a.ch", "M: MODULE\n".to_owned());
        assert_eq!(
            map.format(Span::new(id, 3, 4), "boom"),
            "src/a.ch:1:4: error: boom"
        );
    }

    #[test]
    fn formats_lex_errors_with_their_file() {
        let mut map = SourceMap::default();
        let id = map.add("a.ch", "/*".to_owned());
        let errors = lexer::tokenize_with_source("/*", id).unwrap_err();
        let text = map.format_lex_errors(&errors);
        assert!(text.starts_with("a.ch:1:1: error: unterminated bracketed comment"));
    }
}

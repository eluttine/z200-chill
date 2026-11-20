use crate::lexer::Span;
use crate::source::SourceMap;

/// A diagnostic with a stable phase code. Codes are intentionally coarse while
/// the language is small (`E01xx` lexer, `E02xx` parser, `E04xx` checker,
/// `E05xx` interpreter, `E06xx` manifest/grant files).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub span: Option<Span>,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: &'static str, span: Option<Span>, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
        }
    }
}

pub fn render(sources: &SourceMap, diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| match diagnostic.span {
            Some(span) => sources.format_diagnostic(diagnostic.code, span, &diagnostic.message),
            None => format!("error[{}]: {}", diagnostic.code, diagnostic.message),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_with_and_without_spans() {
        let mut sources = SourceMap::default();
        let id = sources.add("a.ch", "M: MODULE\n".to_owned());

        let diagnostics = vec![
            Diagnostic::new("E0400", Some(Span::new(id, 3, 4)), "boom"),
            Diagnostic::new("E0300", None, "manifest problem"),
        ];

        let text = render(&sources, &diagnostics);
        assert!(text.contains("a.ch:1:4: error[E0400]: boom"));
        assert!(text.contains("error[E0300]: manifest problem"));
    }
}

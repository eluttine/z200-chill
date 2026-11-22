use crate::ast::*;
use crate::lexer::{Keyword, SourceId, Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub span: Span,
    pub message: String,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "E0201",
            span,
            message: message.into(),
        }
    }

    pub fn coded(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub fn parse(tokens: &[Token]) -> Result<Program, Error> {
    let source = tokens
        .first()
        .map(|token| token.span.source)
        .unwrap_or(SourceId(0));
    Parser {
        tokens,
        index: 0,
        source,
        depth: 0,
    }
    .parse_program()
}

pub fn parse_context_piece(tokens: &[Token]) -> Result<Vec<Context>, Error> {
    let source = tokens
        .first()
        .map(|token| token.span.source)
        .unwrap_or(SourceId(0));
    let mut parser = Parser {
        tokens,
        index: 0,
        source,
        depth: 0,
    };
    let contexts = parser.parse_context_list()?;
    if contexts.is_empty() {
        return Err(parser.error_here("expected a CONTEXT piece"));
    }
    if !parser.at_end() {
        return Err(parser.error_here("expected end of context-list piece"));
    }
    Ok(contexts)
}

fn body_span(source: SourceId, start: Span, body: &[Action]) -> Span {
    let end = body
        .last()
        .map(|action| action.span().end)
        .unwrap_or(start.end);
    Span::new(source, start.start, end)
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
    source: SourceId,
    depth: usize,
}

const MAX_NESTING_DEPTH: usize = 128;

impl<'a> Parser<'a> {
    fn parse_program(&mut self) -> Result<Program, Error> {
        let mut modules = Vec::new();
        let mut remotes = Vec::new();
        let mut generic_modules = Vec::new();
        let mut generic_module_instances = Vec::new();
        while !self.at_end() {
            if self.at_context_module_remote() {
                remotes.push(self.parse_context_module_remote()?);
                continue;
            }
            if self.at_remote_unit() {
                remotes.push(self.parse_remote_unit()?);
                continue;
            }
            let contexts = self.parse_context_list()?;
            if self.at_generic_module_template() {
                generic_modules.push(self.parse_generic_module_template(contexts)?);
            } else if self.at_generic_module_instance() {
                generic_module_instances.push(self.parse_generic_module_instance()?);
            } else {
                modules.push(self.parse_module(contexts)?);
            }
        }
        if modules.is_empty()
            && remotes.is_empty()
            && generic_modules.is_empty()
            && generic_module_instances.is_empty()
        {
            return Err(self.error_here("expected a module"));
        }
        Ok(Program {
            modules,
            remotes,
            generic_modules,
            generic_module_instances,
        })
    }

    fn at_context_module_remote(&self) -> bool {
        self.at_keyword(Keyword::Context)
            && self.kind_at(1) == Some(&TokenKind::Keyword(Keyword::Module))
    }

    fn parse_context_module_remote(&mut self) -> Result<RemoteUnit, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Context)?;
        self.expect_keyword(Keyword::Module)?;
        self.expect_keyword(Keyword::Remote)?;
        let designator = self.parse_piece_designator(true)?;
        let end = self.expect_semicolon()?;
        Ok(RemoteUnit {
            kind: RemoteKind::ContextModule,
            name: None,
            designator,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn at_remote_unit(&self) -> bool {
        if self.at_keyword(Keyword::Remote) {
            return true;
        }
        if self.at_keyword(Keyword::Spec)
            && self.kind_at(1) == Some(&TokenKind::Keyword(Keyword::Remote))
        {
            return true;
        }
        if self.at_identifier() && self.kind_at(1) == Some(&TokenKind::Colon) {
            match self.kind_at(2) {
                Some(TokenKind::Keyword(Keyword::Remote)) => true,
                Some(TokenKind::Keyword(Keyword::Spec)) => {
                    self.kind_at(3) == Some(&TokenKind::Keyword(Keyword::Remote))
                }
                _ => false,
            }
        } else {
            false
        }
    }

    fn parse_remote_unit(&mut self) -> Result<RemoteUnit, Error> {
        let start = self.current_span();
        let name = if self.at_identifier() && self.kind_at(1) == Some(&TokenKind::Colon) {
            let (name, _) = self.expect_identifier()?;
            self.expect_colon()?;
            Some(name)
        } else {
            None
        };

        let kind = if self.at_keyword(Keyword::Remote) {
            self.advance();
            if name.is_some() {
                RemoteKind::Module
            } else {
                RemoteKind::ProgramUnit
            }
        } else if self.at_keyword(Keyword::Spec) {
            self.advance();
            self.expect_keyword(Keyword::Remote)?;
            RemoteKind::SpecModule
        } else {
            return Err(self.error_here("expected REMOTE or SPEC REMOTE"));
        };

        let designator = self.parse_piece_designator(true)?;
        let end = self.expect_semicolon()?;
        Ok(RemoteUnit {
            kind,
            name,
            designator,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_piece_designator(&mut self, allow_empty: bool) -> Result<PieceDesignator, Error> {
        let span = self.current_span();
        match self.kind() {
            Some(TokenKind::String(value)) => {
                let value = value.clone();
                self.advance();
                Ok(PieceDesignator {
                    kind: PieceDesignatorKind::Literal(value),
                    span,
                })
            }
            Some(TokenKind::Identifier(name)) => {
                let name = name.clone();
                self.advance();
                Ok(PieceDesignator {
                    kind: PieceDesignatorKind::TextRef(name),
                    span,
                })
            }
            _ if allow_empty => Ok(PieceDesignator {
                kind: PieceDesignatorKind::Empty,
                span,
            }),
            _ => Err(self.expected("a piece designator")),
        }
    }

    fn parse_context_list(&mut self) -> Result<Vec<Context>, Error> {
        let mut contexts = Vec::new();
        while self.at_keyword(Keyword::Context) {
            let start = self.current_span();
            self.advance();
            let remote = if self.at_keyword(Keyword::Remote) {
                self.advance();
                Some(self.parse_piece_designator(true)?)
            } else {
                None
            };
            let declarations = self.parse_declarations(true, true)?;
            for declaration in &declarations {
                match declaration {
                    Declaration::Visibility(visibility) => {
                        if contexts.is_empty() && remote.is_none() {
                            return Err(Error::coded(
                                "E0205",
                                visibility.span,
                                "a context with no enclosing context cannot contain visibility statements",
                            ));
                        }
                    }
                    Declaration::Var(vars) => {
                        if let Some(var) = vars.iter().find(|var| var.init.is_some()) {
                            return Err(Error::coded(
                                "E0205",
                                var.span,
                                "initialization is not allowed in a context",
                            ));
                        }
                    }
                    Declaration::Mode(mode) => {
                        return Err(Error::coded(
                            "E0205",
                            mode.span,
                            "mode definitions in contexts are not implemented yet",
                        ));
                    }
                    Declaration::Proc(_)
                    | Declaration::ProcSignature(_)
                    | Declaration::SpecModule(_)
                    | Declaration::GenericProc(_)
                    | Declaration::GenericProcInstance(_) => {}
                }
            }
            let end = self.expect_keyword(Keyword::For)?;
            contexts.push(Context {
                declarations,
                remote,
                span: Span::new(self.source, start.start, end.end),
            });
        }
        Ok(contexts)
    }

    fn at_generic_module_template(&self) -> bool {
        self.at_identifier()
            && self.kind_at(1) == Some(&TokenKind::Colon)
            && self.kind_at(2) == Some(&TokenKind::Keyword(Keyword::Generic))
    }

    fn at_generic_module_instance(&self) -> bool {
        self.at_identifier()
            && self.kind_at(1) == Some(&TokenKind::Colon)
            && self.kind_at(2) == Some(&TokenKind::Keyword(Keyword::Module))
            && self.kind_at(3) == Some(&TokenKind::Eq)
    }

    fn parse_generic_formals(&mut self) -> Result<Vec<GenericFormal>, Error> {
        let mut formals = Vec::new();
        loop {
            if self.at_keyword(Keyword::Mode) {
                self.advance();
                loop {
                    let (name, _) = self.expect_identifier()?;
                    formals.push(GenericFormal::Mode(name));
                    if self.at_comma() {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect_semicolon()?;
            } else if self.at_keyword(Keyword::Syn) {
                self.advance();
                let mut names = Vec::new();
                loop {
                    let (name, _) = self.expect_identifier()?;
                    names.push(name);
                    if self.at_comma() {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect_equal()?;
                let default = self.parse_expr()?;
                self.expect_semicolon()?;
                for name in names {
                    formals.push(GenericFormal::Syn {
                        name,
                        default: default.clone(),
                    });
                }
            } else if self.at_keyword(Keyword::Proc)
                && self
                    .kind_at(1)
                    .is_some_and(|kind| matches!(kind, TokenKind::Identifier(_)))
            {
                self.advance();
                let (name, _) = self.expect_identifier()?;
                let params = self.parse_formal_params()?;
                let (result, exceptions) = self.parse_result_and_exceptions()?;
                self.expect_semicolon()?;
                formals.push(GenericFormal::Proc {
                    name,
                    params,
                    result,
                    exceptions,
                });
            } else {
                break;
            }
        }
        Ok(formals)
    }

    fn parse_generic_actuals(&mut self) -> Result<Vec<GenericActual>, Error> {
        self.expect_lparen()?;
        let mut actuals = Vec::new();
        if !self.at_rparen() {
            loop {
                if self.at_keyword(Keyword::Proc) {
                    self.advance();
                    let (name, _) = self.expect_identifier()?;
                    self.expect_equal()?;
                    let (target, _) = self.expect_identifier()?;
                    self.expect_semicolon()?;
                    actuals.push(GenericActual::Proc { name, target });
                } else if self.actual_starts_value() {
                    actuals.push(GenericActual::Value(self.parse_expr()?));
                } else {
                    actuals.push(GenericActual::Mode(self.parse_mode_ref()?));
                }
                if self.at_comma() {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect_rparen()?;
        Ok(actuals)
    }

    fn actual_starts_value(&self) -> bool {
        matches!(
            self.kind(),
            Some(
                TokenKind::Integer { .. }
                    | TokenKind::Float(_)
                    | TokenKind::Bool(_)
                    | TokenKind::Char(_)
                    | TokenKind::String(_)
                    | TokenKind::WideChar(_)
                    | TokenKind::WideString(_)
                    | TokenKind::Bits(_)
                    | TokenKind::Minus
                    | TokenKind::LParen
                    | TokenKind::LParenColon
                    | TokenKind::LBracket
            )
        )
    }

    fn parse_generic_module_template(
        &mut self,
        contexts: Vec<Context>,
    ) -> Result<GenericModuleTemplate, Error> {
        let start = self.current_span();
        let (name, _) = self.expect_identifier()?;
        self.expect_colon()?;
        self.expect_keyword(Keyword::Generic)?;
        let formal_params = self.parse_generic_formals()?;
        if !contexts.is_empty() {
            return Err(self.error_here_code(
                "E0206",
                "contexts on generic module templates are not implemented yet",
            ));
        }
        let module = self.parse_module_named(contexts, Some(name.clone()))?;
        Ok(GenericModuleTemplate {
            name,
            formal_params,
            span: Span::new(self.source, start.start, module.span.end),
            module,
        })
    }

    fn parse_generic_module_instance(&mut self) -> Result<GenericModuleInstance, Error> {
        let start = self.current_span();
        let (name, _) = self.expect_identifier()?;
        self.expect_colon()?;
        self.expect_keyword(Keyword::Module)?;
        self.expect_equal()?;
        self.expect_keyword(Keyword::New)?;
        let (template, _) = self.expect_identifier()?;
        let actuals = self.parse_generic_actuals()?;
        self.expect_keyword(Keyword::End)?;
        if let Some(TokenKind::Identifier(trailing)) = self.kind() {
            if trailing != &name {
                return Err(self.error_here_code(
                    "E0202",
                    format!("generic module end name `{trailing}` does not match `{name}`"),
                ));
            }
            self.advance();
        }
        let end = self.expect_semicolon()?;
        Ok(GenericModuleInstance {
            name,
            template,
            actuals,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_module(&mut self, contexts: Vec<Context>) -> Result<Module, Error> {
        self.parse_module_named(contexts, None)
    }

    fn parse_module_named(
        &mut self,
        contexts: Vec<Context>,
        preset_name: Option<String>,
    ) -> Result<Module, Error> {
        let start = self.current_span();

        let name = if let Some(name) = preset_name {
            Some(name)
        } else if self.at_identifier() && self.kind_at(1) == Some(&TokenKind::Colon) {
            let (name, _) = self.expect_identifier()?;
            self.expect_colon()?;
            Some(name)
        } else {
            None
        };

        let kind = if self.at_keyword(Keyword::Spec) {
            self.advance();
            if self.at_keyword(Keyword::Module) {
                self.advance();
                ModuleKind::Spec
            } else if self.at_keyword(Keyword::Region) {
                self.advance();
                ModuleKind::SpecRegion
            } else {
                return Err(self.error_here("expected MODULE or REGION after SPEC"));
            }
        } else if self.at_keyword(Keyword::Module) {
            self.advance();
            if self.at_keyword(Keyword::Spec) {
                self.advance();
                ModuleKind::Spec
            } else {
                if self.at_keyword(Keyword::Body) {
                    self.advance();
                }
                ModuleKind::Body
            }
        } else if self.at_keyword(Keyword::Region) {
            self.advance();
            if self.at_keyword(Keyword::Spec) {
                self.advance();
                ModuleKind::SpecRegion
            } else {
                if self.at_keyword(Keyword::Body) {
                    self.advance();
                }
                ModuleKind::Region
            }
        } else {
            return Err(self.error_here("expected MODULE or REGION"));
        };

        if kind.is_spec() && name.is_none() {
            return Err(self.error_here_code("E0204", "spec module or spec region must be named"));
        }

        let declarations = self.parse_declarations(true, kind.is_spec())?;
        let actions = if kind.is_spec() {
            if !self.at_keyword(Keyword::End) {
                return Err(self.error_here_code(
                    "E0204",
                    "spec module or spec region cannot contain actions",
                ));
            }
            Vec::new()
        } else if kind.is_region() {
            if !self.at_keyword(Keyword::End) {
                return Err(self.error_here_code("E0204", "a region cannot contain actions"));
            }
            Vec::new()
        } else {
            self.parse_actions_until(&[Keyword::End])?
        };
        self.expect_keyword(Keyword::End)?;
        let handler = if self.at_keyword(Keyword::On) {
            Some(self.parse_handler()?)
        } else {
            None
        };
        if kind.is_spec() && handler.is_some() {
            return Err(
                self.error_here_code("E0204", "spec module or spec region cannot have a handler")
            );
        }

        if let Some(TokenKind::Identifier(trailing)) = self.kind() {
            if let Some(expected) = &name {
                if trailing != expected {
                    return Err(self.error_here_code(
                        "E0202",
                        format!("module end name `{trailing}` does not match `{expected}`"),
                    ));
                }
            }
            self.advance();
        }

        let end = self.expect_semicolon()?;
        Ok(Module {
            name,
            kind,
            package: 0,
            package_name: None,
            contexts,
            declarations,
            actions,
            handler,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn at_spec_module_start(&self) -> bool {
        if self.at_keyword(Keyword::Spec)
            && self.kind_at(1) == Some(&TokenKind::Keyword(Keyword::Module))
        {
            return true;
        }
        if self.at_keyword(Keyword::Module)
            && self.kind_at(1) == Some(&TokenKind::Keyword(Keyword::Spec))
        {
            return true;
        }
        if self.at_identifier() && self.kind_at(1) == Some(&TokenKind::Colon) {
            match self.kind_at(2) {
                Some(TokenKind::Keyword(Keyword::Spec)) => {
                    self.kind_at(3) == Some(&TokenKind::Keyword(Keyword::Module))
                }
                Some(TokenKind::Keyword(Keyword::Module)) => {
                    self.kind_at(3) == Some(&TokenKind::Keyword(Keyword::Spec))
                }
                _ => false,
            }
        } else {
            false
        }
    }

    fn parse_declarations(
        &mut self,
        allow_visibility: bool,
        spec: bool,
    ) -> Result<Vec<Declaration>, Error> {
        let mut declarations = Vec::new();
        loop {
            if spec && (self.at_keyword(Keyword::Context) || self.at_spec_module_start()) {
                let contexts = self.parse_context_list()?;
                if !self.at_spec_module_start() {
                    return Err(self.error_here("expected a nested spec module"));
                }
                let module = self.parse_module(contexts)?;
                declarations.push(Declaration::SpecModule(Box::new(module)));
            } else if self.at_keyword(Keyword::Dcl) {
                declarations.push(Declaration::Var(self.parse_dcl()?));
            } else if self.at_keyword(Keyword::NewMode) || self.at_keyword(Keyword::SynMode) {
                declarations.push(Declaration::Mode(self.parse_mode_definition()?));
            } else if self.at_generic_proc_template() {
                declarations.push(Declaration::GenericProc(
                    self.parse_generic_proc_template()?,
                ));
            } else if self.at_generic_proc_instance() {
                declarations.push(Declaration::GenericProcInstance(
                    self.parse_generic_proc_instance()?,
                ));
            } else if self.at_proc_definition() {
                if spec {
                    declarations.push(Declaration::ProcSignature(self.parse_proc_signature()?));
                } else {
                    declarations.push(Declaration::Proc(self.parse_proc()?));
                }
            } else if self.at_keyword(Keyword::Grant) {
                if !allow_visibility {
                    return Err(
                        self.error_here_code("E0205", "GRANT is only allowed at module level")
                    );
                }
                declarations.push(Declaration::Visibility(
                    self.parse_visibility(VisibilityKind::Grant)?,
                ));
            } else if self.at_keyword(Keyword::Seize) {
                if !allow_visibility {
                    return Err(
                        self.error_here_code("E0205", "SEIZE is only allowed at module level")
                    );
                }
                declarations.push(Declaration::Visibility(
                    self.parse_visibility(VisibilityKind::Seize)?,
                ));
            } else {
                break;
            }
        }
        Ok(declarations)
    }

    fn parse_visibility(&mut self, kind: VisibilityKind) -> Result<Visibility, Error> {
        let start = self.current_span();
        self.advance(); // GRANT or SEIZE

        let mut prefix = None;

        if self.at_lparen() {
            self.advance(); // (
            let old = if self.at_arrow() {
                Vec::new()
            } else {
                self.parse_prefix_parts()?
            };
            self.expect_arrow()?;
            let new = if self.at_rparen() {
                Vec::new()
            } else {
                self.parse_prefix_parts()?
            };
            self.expect_rparen()?;
            self.expect_bang()?;
            prefix = Some(PrefixClause::Rename { old, new });
        }

        let mut names = Vec::new();
        let mut all = false;
        let mut friends = Vec::new();
        let mut forbid = None;

        if self.at_keyword(Keyword::All) {
            self.advance();
            all = true;
        } else {
            loop {
                names.push(self.parse_name_ref()?);
                if self.at_comma() {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        if self.at_keyword(Keyword::Forbid) {
            if kind == VisibilityKind::Seize {
                return Err(self.error_here_code("E0205", "SEIZE does not accept FORBID"));
            }
            self.advance();
            if self.at_keyword(Keyword::All) {
                self.advance();
                forbid = Some(Forbid {
                    fields: Vec::new(),
                    all: true,
                });
            } else {
                self.expect_lparen()?;
                let mut fields = Vec::new();
                if !self.at_rparen() {
                    loop {
                        let (field, _) = self.expect_identifier()?;
                        fields.push(field);
                        if self.at_comma() {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect_rparen()?;
                forbid = Some(Forbid { fields, all: false });
            }
        }

        if self.at_keyword(Keyword::Prefixed) {
            self.advance();
            let parts = if self.at_identifier() {
                Some(self.parse_prefix_parts()?)
            } else {
                None
            };
            prefix = Some(PrefixClause::Prefixed(parts));
        }

        if self.at_keyword(Keyword::To) {
            if kind == VisibilityKind::Seize {
                return Err(self.error_here_code("E0205", "SEIZE does not accept TO"));
            }
            self.advance();
            loop {
                friends.push(self.parse_name_ref()?);
                if self.at_comma() {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let end = self.expect_semicolon()?;
        Ok(Visibility {
            kind,
            names,
            all,
            friends,
            prefix,
            forbid,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_prefix_parts(&mut self) -> Result<Vec<String>, Error> {
        let mut parts = vec![self.expect_identifier()?.0];
        while matches!(self.kind(), Some(TokenKind::Bang)) {
            self.advance();
            parts.push(self.expect_identifier()?.0);
        }
        Ok(parts)
    }

    fn parse_name_ref(&mut self) -> Result<NameRef, Error> {
        let start = self.current_span();
        let (first, first_span) = self.expect_identifier()?;
        let mut prefix = Vec::new();
        let mut name = first;
        let mut end = first_span.end;

        while matches!(self.kind(), Some(TokenKind::Bang)) {
            self.advance();
            let (next, span) = self.expect_identifier()?;
            prefix.push(name);
            name = next;
            end = span.end;
        }

        Ok(NameRef {
            prefix,
            name,
            span: Span::new(self.source, start.start, end),
        })
    }

    fn parse_mode_definition(&mut self) -> Result<ModeDefinition, Error> {
        let start = self.current_span();
        self.advance(); // NEWMODE or SYNMODE
        let (name, _) = self.expect_identifier()?;
        self.expect_equal()?;

        let body = if self.at_keyword(Keyword::Struct) {
            self.advance();
            self.expect_lparen()?;
            let mut fields = Vec::new();
            if !self.at_rparen() {
                loop {
                    let (field_name, field_span) = self.expect_identifier()?;
                    let mode = self.parse_mode_ref()?;
                    let end = mode.span.end;
                    fields.push(FieldDef {
                        name: field_name,
                        mode,
                        span: Span::new(self.source, field_span.start, end),
                    });
                    if self.at_comma() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect_rparen()?;
            ModeDefinitionBody::Struct(fields)
        } else if self.at_keyword(Keyword::Set) {
            self.advance();
            self.expect_lparen()?;
            let mut elements = Vec::new();
            if !self.at_rparen() {
                loop {
                    let (element_name, element_span) = self.expect_identifier()?;
                    let value = if matches!(self.kind(), Some(TokenKind::Eq)) {
                        self.advance();
                        let value = self.parse_mode_integer()?;
                        if value < 0 {
                            return Err(Error::coded(
                                "E0205",
                                element_span,
                                "set element representation must be non-negative",
                            ));
                        }
                        Some(value)
                    } else {
                        None
                    };
                    elements.push(SetElement {
                        name: element_name,
                        value,
                        span: element_span,
                    });
                    if self.at_comma() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect_rparen()?;
            ModeDefinitionBody::Set(elements)
        } else if self.at_keyword(Keyword::Module) {
            let body_start = self.current_span();
            self.advance();
            let spec = if self.at_keyword(Keyword::Spec) {
                self.advance();
                true
            } else {
                if self.at_keyword(Keyword::Body) {
                    self.advance();
                }
                false
            };
            let mut bases = Vec::new();
            if self.at_keyword(Keyword::BasedOn) {
                self.advance();
                loop {
                    let (base, _) = self.expect_identifier()?;
                    bases.push(base);
                    if self.at_comma() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            let declarations = self.parse_declarations(spec, spec)?;
            let invariant = if self.at_keyword(Keyword::Invariant) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect_keyword(Keyword::End)?;
            if let Some(TokenKind::Identifier(trailing)) = self.kind() {
                if trailing != &name {
                    return Err(self.error_here_code(
                        "E0202",
                        format!("module mode end name `{trailing}` does not match `{name}`"),
                    ));
                }
                self.advance();
            }
            ModeDefinitionBody::Moreta {
                spec,
                bases,
                declarations,
                invariant,
                span: Span::new(self.source, body_start.start, self.current_span().start),
            }
        } else {
            ModeDefinitionBody::Alias(self.parse_mode_ref()?)
        };

        let end = self.expect_semicolon()?;
        Ok(ModeDefinition {
            name,
            body,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_dcl(&mut self) -> Result<Vec<VarDecl>, Error> {
        self.expect_keyword(Keyword::Dcl)?;
        let mut declarations = Vec::new();

        loop {
            let mut names = vec![self.expect_identifier()?];

            // Before a mode is parsed, a comma continues the defining-occurrence list.
            while self.at_comma() {
                self.advance();
                names.push(self.expect_identifier()?);
            }

            let mode = self.parse_mode_ref()?;
            if self.at_keyword(Keyword::Static) {
                self.advance();
            }

            let init = if self.at_keyword(Keyword::Init) {
                self.advance();
                self.expect_assign()?;
                Some(Initialization {
                    kind: InitKind::LifetimeBound,
                    value: self.parse_expr()?,
                })
            } else if self.at_assign() {
                self.advance();
                Some(Initialization {
                    kind: InitKind::ReachBound,
                    value: self.parse_expr()?,
                })
            } else {
                None
            };

            let end = init
                .as_ref()
                .map(|init| init.value.span().end)
                .unwrap_or(mode.span.end);

            for (name, name_span) in names {
                declarations.push(VarDecl {
                    name,
                    mode: mode.clone(),
                    init: init.clone(),
                    span: Span::new(self.source, name_span.start, end),
                });
            }

            if self.at_comma() {
                self.advance();
                continue;
            }

            self.expect_semicolon()?;
            break;
        }

        Ok(declarations)
    }

    fn at_generic_proc_template(&self) -> bool {
        self.at_identifier()
            && self.kind_at(1) == Some(&TokenKind::Colon)
            && self.kind_at(2) == Some(&TokenKind::Keyword(Keyword::Generic))
    }

    fn at_generic_proc_instance(&self) -> bool {
        self.at_identifier()
            && self.kind_at(1) == Some(&TokenKind::Colon)
            && self.kind_at(2) == Some(&TokenKind::Keyword(Keyword::Proc))
            && self.kind_at(3) == Some(&TokenKind::Eq)
    }

    fn parse_generic_proc_template(&mut self) -> Result<GenericProcTemplate, Error> {
        let start = self.current_span();
        let (name, _) = self.expect_identifier()?;
        self.expect_colon()?;
        self.expect_keyword(Keyword::Generic)?;
        let formal_params = self.parse_generic_formals()?;
        if self.at_keyword(Keyword::Proc) && self.kind_at(1) == Some(&TokenKind::Colon) {
            return Err(
                self.error_here_code("E0206", "generic PROC parameters are not implemented yet")
            );
        }
        self.expect_keyword(Keyword::Proc)?;
        let params = self.parse_formal_params()?;
        let (result, exceptions) = self.parse_result_and_exceptions()?;
        self.expect_semicolon()?;
        let declarations = self.parse_declarations(false, false)?;
        let actions = self.parse_actions_until(&[Keyword::End])?;
        self.expect_keyword(Keyword::End)?;
        let handler = if self.at_keyword(Keyword::On) {
            Some(self.parse_handler()?)
        } else {
            None
        };
        if let Some(TokenKind::Identifier(trailing)) = self.kind() {
            if trailing != &name {
                return Err(self.error_here_code(
                    "E0202",
                    format!("generic procedure end name `{trailing}` does not match `{name}`"),
                ));
            }
            self.advance();
        }
        let end = self.expect_semicolon()?;
        let span = Span::new(self.source, start.start, end.end);
        Ok(GenericProcTemplate {
            name: name.clone(),
            formal_params,
            proc_def: ProcDef {
                name,
                params,
                result,
                exceptions,
                declarations,
                actions,
                handler,
                span,
            },
            span,
        })
    }

    fn parse_generic_proc_instance(&mut self) -> Result<GenericProcInstance, Error> {
        let start = self.current_span();
        let (name, _) = self.expect_identifier()?;
        self.expect_colon()?;
        self.expect_keyword(Keyword::Proc)?;
        self.expect_equal()?;
        self.expect_keyword(Keyword::New)?;
        let (template, _) = self.expect_identifier()?;
        let actuals = self.parse_generic_actuals()?;
        self.expect_keyword(Keyword::End)?;
        if let Some(TokenKind::Identifier(trailing)) = self.kind() {
            if trailing != &name {
                return Err(self.error_here_code(
                    "E0202",
                    format!("generic instance end name `{trailing}` does not match `{name}`"),
                ));
            }
            self.advance();
        }
        let end = self.expect_semicolon()?;
        Ok(GenericProcInstance {
            name,
            template,
            actuals,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_proc(&mut self) -> Result<ProcDef, Error> {
        let start = self.current_span();
        let (name, _) = self.expect_identifier()?;
        self.expect_colon()?;
        self.expect_keyword(Keyword::Proc)?;
        let params = self.parse_formal_params()?;
        let (result, exceptions) = self.parse_result_and_exceptions()?;

        self.expect_semicolon()?;
        let declarations = self.parse_declarations(false, false)?;
        let actions = self.parse_actions_until(&[Keyword::End])?;
        self.expect_keyword(Keyword::End)?;
        let handler = if self.at_keyword(Keyword::On) {
            Some(self.parse_handler()?)
        } else {
            None
        };

        if let Some(TokenKind::Identifier(trailing)) = self.kind() {
            if trailing != &name {
                return Err(self.error_here_code(
                    "E0202",
                    format!("procedure end name `{trailing}` does not match `{name}`"),
                ));
            }
            self.advance();
        }

        let end = self.expect_semicolon()?;
        Ok(ProcDef {
            name,
            params,
            result,
            exceptions,
            declarations,
            actions,
            handler,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_proc_signature(&mut self) -> Result<ProcSignature, Error> {
        let start = self.current_span();
        let (name, _) = self.expect_identifier()?;
        self.expect_colon()?;
        self.expect_keyword(Keyword::Proc)?;
        let params = self.parse_formal_params()?;
        let (result, exceptions) = self.parse_result_and_exceptions()?;

        self.expect_keyword(Keyword::End)?;
        if let Some(TokenKind::Identifier(trailing)) = self.kind() {
            if trailing != &name {
                return Err(self.error_here_code(
                    "E0202",
                    format!("procedure signature end name `{trailing}` does not match `{name}`"),
                ));
            }
            self.advance();
        }
        let end = self.expect_semicolon()?;

        Ok(ProcSignature {
            name,
            params,
            result,
            exceptions,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_formal_params(&mut self) -> Result<Vec<Param>, Error> {
        self.expect_lparen()?;

        let mut params = Vec::new();
        if !self.at_rparen() {
            loop {
                let mut names = vec![self.expect_identifier()?];

                // As in DCL, a comma before the mode continues the name list.
                while self.at_comma() {
                    self.advance();
                    names.push(self.expect_identifier()?);
                }

                let mode = self.parse_mode_ref()?;
                let passing = self.parse_passing()?;
                let param_end = mode.span.end;

                for (param_name, name_span) in names {
                    params.push(Param {
                        name: param_name,
                        mode: mode.clone(),
                        passing,
                        span: Span::new(self.source, name_span.start, param_end),
                    });
                }

                if self.at_comma() {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect_rparen()?;

        Ok(params)
    }

    fn parse_result_and_exceptions(&mut self) -> Result<(Option<ModeRef>, Vec<String>), Error> {
        let result = if self.at_keyword(Keyword::Returns) {
            self.advance();
            self.expect_lparen()?;
            let mode = self.parse_mode_ref()?;
            if self.at_keyword(Keyword::Loc) {
                self.advance();
            }
            self.expect_rparen()?;
            Some(mode)
        } else {
            None
        };

        let mut exceptions = Vec::new();
        if self.at_keyword(Keyword::Exceptions) {
            self.advance();
            self.expect_lparen()?;
            if !self.at_rparen() {
                loop {
                    let (exception, _) = self.expect_identifier()?;
                    exceptions.push(exception);
                    if self.at_comma() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect_rparen()?;
        }

        Ok((result, exceptions))
    }

    fn parse_passing(&mut self) -> Result<Passing, Error> {
        let passing = match self.kind() {
            Some(TokenKind::Keyword(Keyword::In)) => Passing::In,
            Some(TokenKind::Keyword(Keyword::Out)) => Passing::Out,
            Some(TokenKind::Keyword(Keyword::InOut)) => Passing::InOut,
            Some(TokenKind::Keyword(Keyword::Loc)) => Passing::Loc,
            _ => return Ok(Passing::In),
        };
        self.advance();

        if passing == Passing::Loc && self.at_keyword(Keyword::Dynamic) {
            self.advance();
        }

        Ok(passing)
    }

    fn parse_mode_ref(&mut self) -> Result<ModeRef, Error> {
        if self.at_keyword(Keyword::Access) {
            let start = self.current_span();
            self.advance();
            let mut access_indexed = false;
            let mut access_index = None;
            if self.at_lparen() {
                self.advance();
                access_indexed = true;
                if matches!(
                    self.kind(),
                    Some(TokenKind::Integer { .. } | TokenKind::Minus)
                ) {
                    let low = self.parse_mode_integer()?;
                    self.expect_colon()?;
                    let high = self.parse_mode_integer()?;
                    if low > high {
                        return Err(Error::coded(
                            "E0205",
                            start,
                            "ACCESS index range lower bound exceeds upper bound",
                        ));
                    }
                    access_index = Some(ArrayBounds { low, high });
                } else {
                    let (name, span) = self.expect_identifier()?;
                    if !matches!(name.as_str(), "INT" | "SHORT_INT" | "LONG_INT") {
                        return Err(Error::coded(
                            "E0205",
                            span,
                            "ACCESS index mode must be an integer mode or literal range",
                        ));
                    }
                }
                self.expect_rparen()?;
            }
            let record = self.parse_mode_ref()?;
            if record.name != "CHARS" || !record.arrays.is_empty() {
                return Err(Error::coded(
                    "E0205",
                    record.span,
                    "ACCESS record mode must be a character string mode",
                ));
            }
            return Ok(ModeRef {
                name: "ACCESS".to_owned(),
                arrays: Vec::new(),
                string_length: None,
                varying: false,
                access: true,
                powerset: false,
                reference: false,
                row: false,
                access_indexed,
                access_index,
                span: Span::new(self.source, start.start, record.span.end),
            });
        }
        if self.at_keyword(Keyword::Powerset) {
            let start = self.current_span();
            self.advance();
            let element = self.parse_mode_ref()?;
            if !element.arrays.is_empty()
                || element.access
                || element.powerset
                || element.string_length.is_some()
            {
                return Err(Error::coded(
                    "E0205",
                    element.span,
                    "POWERSET member mode must be a scalar mode",
                ));
            }
            return Ok(ModeRef {
                name: element.name,
                arrays: Vec::new(),
                string_length: None,
                varying: false,
                access: false,
                powerset: true,
                reference: false,
                row: false,
                access_indexed: false,
                access_index: None,
                span: Span::new(self.source, start.start, element.span.end),
            });
        }
        if self.at_keyword(Keyword::Array) {
            let start = self.current_span();
            self.advance();
            self.expect_lparen()?;
            let mut arrays = Vec::new();
            loop {
                let low = self.parse_mode_integer()?;
                self.expect_colon()?;
                let high = self.parse_mode_integer()?;
                if low > high {
                    return Err(Error::coded(
                        "E0205",
                        start,
                        "array lower bound is greater than its upper bound",
                    ));
                }
                arrays.push(ArrayBounds { low, high });
                if self.at_comma() {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect_rparen()?;
            if arrays.len() > crate::types::MAX_ARRAY_RANK {
                return Err(Error::coded(
                    "E0205",
                    start,
                    format!(
                        "arrays with more than {} dimensions are not implemented yet",
                        crate::types::MAX_ARRAY_RANK
                    ),
                ));
            }
            let element = self.parse_mode_ref()?;
            if !element.arrays.is_empty() {
                return Err(Error::coded(
                    "E0205",
                    element.span,
                    "nested array modes are not implemented yet",
                ));
            }
            return Ok(ModeRef {
                name: element.name,
                arrays,
                string_length: element.string_length,
                varying: element.varying,
                access: false,
                powerset: false,
                reference: false,
                row: false,
                access_indexed: false,
                access_index: None,
                span: Span::new(self.source, start.start, element.span.end),
            });
        }
        if self.at_keyword(Keyword::Chars)
            || self.at_keyword(Keyword::WChars)
            || self.at_keyword(Keyword::Bools)
        {
            let kind = if self.at_keyword(Keyword::WChars) {
                "WCHARS"
            } else if self.at_keyword(Keyword::Bools) {
                "BOOLS"
            } else {
                "CHARS"
            };
            let span = self.current_span();
            self.advance();
            let length = if self.at_lparen() {
                self.advance();
                let length = self.parse_mode_integer()?;
                self.expect_rparen()?;
                if length < 0 {
                    return Err(Error::coded(
                        "E0205",
                        span,
                        "string length must be non-negative",
                    ));
                }
                Some(length as usize)
            } else {
                None
            };
            let varying = if self.at_keyword(Keyword::Varying) {
                self.advance();
                if length.is_none() {
                    return Err(Error::coded(
                        "E0205",
                        span,
                        match kind {
                            "WCHARS" => "VARYING requires a WCHARS(n) length",
                            "BOOLS" => "VARYING requires a BOOLS(n) length",
                            _ => "VARYING requires a CHARS(n) length",
                        },
                    ));
                }
                true
            } else {
                false
            };
            return Ok(ModeRef {
                name: kind.to_owned(),
                arrays: Vec::new(),
                string_length: length,
                varying,
                access: false,
                powerset: false,
                reference: false,
                row: false,
                access_indexed: false,
                access_index: None,
                span,
            });
        }
        if self.at_keyword(Keyword::Text) || self.at_keyword(Keyword::WText) {
            let wide = self.at_keyword(Keyword::WText);
            let span = self.current_span();
            self.advance();
            self.expect_lparen()?;
            let length = self.parse_mode_integer()?;
            self.expect_rparen()?;
            if length < 0 {
                return Err(Error::coded(
                    "E0205",
                    span,
                    "text length must be non-negative",
                ));
            }
            // Optional index mode and DYNAMIC are accepted for compatibility with
            // the surface syntax. The interpreter represents a text location by
            // its varying text record mode and does not keep a separate index
            // sub-mode yet.
            if let Some(TokenKind::Identifier(name)) = self.kind() {
                if matches!(name.as_str(), "INT" | "SHORT_INT" | "LONG_INT") {
                    self.advance();
                }
            }
            if self.at_keyword(Keyword::Dynamic) {
                self.advance();
            }
            return Ok(ModeRef {
                name: if wide { "WTEXT" } else { "TEXT" }.to_owned(),
                arrays: Vec::new(),
                string_length: Some(length as usize),
                varying: true,
                access: false,
                powerset: false,
                reference: false,
                row: false,
                access_indexed: false,
                access_index: None,
                span,
            });
        }
        if self.at_keyword(Keyword::Ref) || self.at_keyword(Keyword::Row) {
            let row = self.at_keyword(Keyword::Row);
            let start = self.current_span();
            self.advance();
            let mut inner = self.parse_mode_ref()?;
            if inner.reference || inner.row {
                return Err(Error::coded(
                    "E0205",
                    inner.span,
                    "nested REF/ROW modes are not implemented yet",
                ));
            }
            inner.reference = !row;
            inner.row = row;
            inner.span = Span::new(self.source, start.start, inner.span.end);
            return Ok(inner);
        }
        let (name, span) = self.expect_identifier()?;
        Ok(ModeRef {
            name,
            arrays: Vec::new(),
            string_length: None,
            varying: false,
            access: false,
            powerset: false,
            reference: false,
            row: false,
            access_indexed: false,
            access_index: None,
            span,
        })
    }

    fn parse_mode_integer(&mut self) -> Result<i128, Error> {
        let negative = if matches!(self.kind(), Some(TokenKind::Minus)) {
            self.advance();
            true
        } else {
            false
        };
        let Some(token) = self.current() else {
            return Err(self.expected("an integer array bound"));
        };
        let value = match &token.kind {
            TokenKind::Integer { value, .. } => *value,
            _ => return Err(self.expected("an integer array bound")),
        };
        self.advance();
        if negative {
            value
                .checked_neg()
                .ok_or_else(|| self.error_here_code("E0205", "array bound is out of range"))
        } else {
            Ok(value)
        }
    }

    fn parse_actions_until(&mut self, stop: &[Keyword]) -> Result<Vec<Action>, Error> {
        let mut actions = Vec::new();
        while !self.at_end() && !stop.iter().any(|keyword| self.at_keyword(*keyword)) {
            actions.push(self.parse_action()?);
        }
        Ok(actions)
    }

    /// Consume the statement terminator and, when an `ON` handler follows,
    /// append it to the action (Z.200 8.2).
    fn finish_action(&mut self, action: Action, start: Span) -> Result<Action, Error> {
        if self.at_keyword(Keyword::On) {
            let handler = self.parse_handler()?;
            let end = self.expect_semicolon()?;
            return Ok(Action::Handled {
                action: Box::new(action),
                handler,
                span: Span::new(self.source, start.start, end.end),
            });
        }
        self.expect_semicolon()?;
        Ok(action)
    }

    fn parse_action(&mut self) -> Result<Action, Error> {
        self.enter_nesting()?;
        let result = self.parse_action_inner();
        self.leave_nesting();
        result
    }

    fn parse_action_inner(&mut self) -> Result<Action, Error> {
        if self.at_identifier() && self.kind_at(1) == Some(&TokenKind::Colon) {
            let start = self.current_span();
            let (label, _) = self.expect_identifier()?;
            self.expect_colon()?;
            if self.at_keyword(Keyword::Module) || self.at_keyword(Keyword::Context) {
                let contexts = self.parse_context_list()?;
                let module = self.parse_module_named(contexts, Some(label))?;
                if module.kind == ModuleKind::Spec {
                    return Err(Error::coded(
                        "E0204",
                        module.span,
                        "nested spec modules are not supported",
                    ));
                }
                return Ok(Action::Module(Box::new(module)));
            }
            let action = self.parse_action()?;
            return Ok(Action::Labelled {
                label,
                span: Span::new(self.source, start.start, action.span().end),
                action: Box::new(action),
            });
        }

        match self.kind() {
            Some(TokenKind::Keyword(Keyword::Context))
            | Some(TokenKind::Keyword(Keyword::Module)) => {
                let contexts = self.parse_context_list()?;
                let module = self.parse_module_named(contexts, None)?;
                if module.kind == ModuleKind::Spec {
                    return Err(Error::coded(
                        "E0204",
                        module.span,
                        "nested spec modules are not supported",
                    ));
                }
                Ok(Action::Module(Box::new(module)))
            }
            Some(TokenKind::Keyword(Keyword::Assert)) => self.parse_assert(),
            Some(TokenKind::Keyword(Keyword::Return)) => self.parse_return(),
            Some(TokenKind::Keyword(Keyword::Result)) => self.parse_result(),
            Some(TokenKind::Keyword(Keyword::Exit)) => self.parse_exit(),
            Some(TokenKind::Keyword(Keyword::Cause)) => self.parse_cause(),
            Some(TokenKind::Keyword(Keyword::If)) => self.parse_if(),
            Some(TokenKind::Keyword(Keyword::Case)) => self.parse_case(),
            Some(TokenKind::Keyword(Keyword::Do)) => self.parse_do(),
            Some(TokenKind::Keyword(Keyword::Begin)) => self.parse_begin(),
            Some(TokenKind::Semicolon) => {
                let span = self.current_span();
                self.advance();
                Ok(Action::Empty { span })
            }
            _ => self.parse_expression_action(),
        }
    }

    fn parse_cause(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Cause)?;
        let (name, _) = self.expect_identifier()?;
        let end = self.expect_semicolon()?;
        Ok(Action::Cause {
            name,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_handler(&mut self) -> Result<Handler, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::On)?;

        let mut alternatives = Vec::new();
        while self.at_lparen() {
            let alternative_start = self.current_span();
            self.advance();
            let mut exceptions = Vec::new();
            if !self.at_rparen() {
                loop {
                    let (name, _) = self.expect_identifier()?;
                    exceptions.push(name);
                    if self.at_comma() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect_rparen()?;
            self.expect_colon()?;
            let actions = self.parse_handler_actions()?;
            alternatives.push(HandlerAlternative {
                exceptions,
                actions,
                span: Span::new(
                    self.source,
                    alternative_start.start,
                    self.handler_actions_end(),
                ),
            });
        }

        let else_actions = if self.at_keyword(Keyword::Else) {
            self.advance();
            Some(self.parse_actions_until(&[Keyword::End])?)
        } else {
            None
        };
        let end = self.expect_keyword(Keyword::End)?;
        Ok(Handler {
            alternatives,
            else_actions,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_handler_actions(&mut self) -> Result<Vec<Action>, Error> {
        let mut actions = Vec::new();
        while !self.at_end()
            && !self.at_lparen()
            && !self.at_keyword(Keyword::Else)
            && !self.at_keyword(Keyword::End)
        {
            actions.push(self.parse_action()?);
        }
        Ok(actions)
    }

    fn handler_actions_end(&self) -> usize {
        self.current_span().start
    }

    fn parse_assert(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Assert)?;
        let condition = self.parse_expr()?;
        let action = Action::Assert {
            condition,
            span: Span::new(self.source, start.start, self.current_span().start),
        };
        self.finish_action(action, start)
    }

    fn parse_return(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Return)?;
        let value = if self.at_semicolon() {
            None
        } else {
            Some(self.parse_expr()?)
        };
        let end = self.expect_semicolon()?;
        Ok(Action::Return {
            value,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_result(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Result)?;
        let value = self.parse_expr()?;
        let end = self.expect_semicolon()?;
        Ok(Action::Result {
            value,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_exit(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Exit)?;
        let label = if let Some(TokenKind::Identifier(label)) = self.kind() {
            let label = label.clone();
            self.advance();
            Some(label)
        } else {
            None
        };
        let end = self.expect_semicolon()?;
        Ok(Action::Exit {
            label,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_if(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::If)?;

        let condition = self.parse_expr()?;
        let branch_start = condition.span();
        self.expect_keyword(Keyword::Then)?;
        let body = self.parse_actions_until(&[Keyword::Elsif, Keyword::Else, Keyword::Fi])?;
        let mut branches = vec![Branch {
            condition,
            span: body_span(self.source, branch_start, &body),
            body,
        }];

        while self.at_keyword(Keyword::Elsif) {
            self.advance();
            let condition = self.parse_expr()?;
            let branch_start = condition.span();
            self.expect_keyword(Keyword::Then)?;
            let body = self.parse_actions_until(&[Keyword::Elsif, Keyword::Else, Keyword::Fi])?;
            branches.push(Branch {
                condition,
                span: body_span(self.source, branch_start, &body),
                body,
            });
        }

        let mut else_span = None;
        let else_actions = if self.at_keyword(Keyword::Else) {
            let start = self.current_span();
            self.advance();
            let actions = self.parse_actions_until(&[Keyword::Fi])?;
            else_span = Some(body_span(self.source, start, &actions));
            Some(actions)
        } else {
            None
        };

        self.expect_keyword(Keyword::Fi)?;
        let action = Action::If {
            branches,
            else_actions,
            else_span,
            span: Span::new(self.source, start.start, self.current_span().start),
        };
        self.finish_action(action, start)
    }

    fn parse_case(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Case)?;
        let selector = self.parse_expr()?;

        if self.at_comma() {
            return Err(
                self.error_here_code("E0206", "multiple CASE selectors are not implemented yet")
            );
        }

        self.expect_keyword(Keyword::Of)?;

        let mut alternatives = Vec::new();
        while self.at_lparen() {
            let alternative_start = self.current_span();
            let labels = self.parse_case_labels()?;
            self.expect_colon()?;
            let body = self.parse_case_body()?;
            let end = body
                .last()
                .map(|action| action.span().end)
                .unwrap_or(alternative_start.end);
            alternatives.push(CaseAlternative {
                labels,
                body,
                span: Span::new(self.source, alternative_start.start, end),
            });
        }

        let mut else_span = None;
        let else_actions = if self.at_keyword(Keyword::Else) {
            let start = self.current_span();
            self.advance();
            let actions = self.parse_actions_until(&[Keyword::Esac])?;
            else_span = Some(body_span(self.source, start, &actions));
            Some(actions)
        } else {
            None
        };

        self.expect_keyword(Keyword::Esac)?;

        if alternatives.is_empty() && else_actions.is_none() {
            return Err(Error::coded(
                "E0206",
                start,
                "CASE requires at least one alternative",
            ));
        }

        let action = Action::Case {
            selector,
            alternatives,
            else_actions,
            else_span,
            span: Span::new(self.source, start.start, self.current_span().start),
        };
        self.finish_action(action, start)
    }

    fn parse_case_body(&mut self) -> Result<Vec<Action>, Error> {
        let mut actions = Vec::new();
        while !self.at_end()
            && !self.at_keyword(Keyword::Esac)
            && !self.at_keyword(Keyword::Else)
            && !self.at_lparen()
        {
            actions.push(self.parse_action()?);
        }
        Ok(actions)
    }

    fn parse_case_labels(&mut self) -> Result<Vec<CaseLabel>, Error> {
        self.expect_lparen()?;

        let mut labels = Vec::new();
        loop {
            if self.at_keyword(Keyword::Else) {
                self.advance();
                labels.push(CaseLabel::Else);
            } else if matches!(self.kind(), Some(TokenKind::Star)) {
                self.advance();
                labels.push(CaseLabel::Any);
            } else {
                let low = self.parse_case_integer()?;
                if matches!(self.kind(), Some(TokenKind::Colon)) {
                    self.advance();
                    let high = self.parse_case_integer()?;
                    if low > high {
                        return Err(self.error_here_code(
                            "E0206",
                            "CASE range lower bound exceeds upper bound",
                        ));
                    }
                    labels.push(CaseLabel::Range { low, high });
                } else {
                    labels.push(CaseLabel::Value(low));
                }
            }

            if self.at_comma() {
                self.advance();
            } else {
                break;
            }
        }

        self.expect_rparen()?;
        Ok(labels)
    }

    fn parse_case_integer(&mut self) -> Result<i128, Error> {
        let negative = if matches!(self.kind(), Some(TokenKind::Minus)) {
            self.advance();
            true
        } else {
            false
        };

        let Some(token) = self.current() else {
            return Err(self.expected("an integer literal"));
        };
        let value = match &token.kind {
            TokenKind::Integer { value, .. } => *value,
            _ => return Err(self.expected("an integer literal")),
        };
        self.advance();

        if negative {
            value
                .checked_neg()
                .ok_or_else(|| self.error_here_code("E0206", "CASE label is out of range"))
        } else {
            Ok(value)
        }
    }

    fn parse_do(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Do)?;

        let control = if self.at_keyword(Keyword::While) {
            self.advance();
            DoControl::While(self.parse_expr()?)
        } else if self.at_keyword(Keyword::For) {
            self.advance();
            if self.at_keyword(Keyword::Ever) {
                self.advance();
                DoControl::ForEver
            } else {
                let (counter, _) = self.expect_identifier()?;
                self.expect_assign()?;
                let start_value = self.parse_expr()?;
                let step = if self.at_keyword(Keyword::By) {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                let down = if self.at_keyword(Keyword::Down) {
                    self.advance();
                    true
                } else {
                    false
                };
                self.expect_keyword(Keyword::To)?;
                let end_value = self.parse_expr()?;
                DoControl::For {
                    counter,
                    start: start_value,
                    end: end_value,
                    step,
                    down,
                }
            }
        } else {
            DoControl::None
        };

        if !matches!(&control, DoControl::None) {
            self.expect_semicolon()?;
        }

        let body = self.parse_actions_until(&[Keyword::Od])?;
        self.expect_keyword(Keyword::Od)?;
        let action = Action::Do {
            control,
            body,
            span: Span::new(self.source, start.start, self.current_span().start),
        };
        self.finish_action(action, start)
    }

    fn parse_begin(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Begin)?;
        let declarations = self.parse_declarations(false, false)?;
        let actions = self.parse_actions_until(&[Keyword::End])?;
        self.expect_keyword(Keyword::End)?;
        let action = Action::Begin {
            declarations,
            actions,
            span: Span::new(self.source, start.start, self.current_span().start),
        };
        self.finish_action(action, start)
    }

    fn parse_expression_action(&mut self) -> Result<Action, Error> {
        let target = self.parse_expr()?;

        if let Some(op) = self.assignment_operator() {
            let start = target.span().start;
            self.advance();
            let value = self.parse_expr()?;
            let value_end = value.span().end;
            let action = Action::Assign {
                target,
                op,
                value,
                span: Span::new(self.source, start, value_end),
            };
            return self.finish_action(action, Span::new(self.source, start, value_end));
        }

        let span = target.span();
        let callee = match target {
            Expr::Apply { .. } | Expr::MethodCall { .. } => target,
            _ => return Err(Error::new(span, "expected assignment or procedure call")),
        };
        self.finish_action(Action::Call { callee, span }, span)
    }

    fn assignment_operator(&self) -> Option<AssignOp> {
        match self.kind()? {
            TokenKind::Assign => Some(AssignOp::Assign),
            TokenKind::PlusAssign => Some(AssignOp::Add),
            TokenKind::MinusAssign => Some(AssignOp::Sub),
            TokenKind::StarAssign => Some(AssignOp::Mul),
            TokenKind::SlashAssign => Some(AssignOp::Div),
            TokenKind::ConcatAssign => Some(AssignOp::Concat),
            _ => None,
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, Error> {
        self.enter_nesting()?;
        let result = self.parse_expr_inner();
        self.leave_nesting();
        result
    }

    fn parse_expr_inner(&mut self) -> Result<Expr, Error> {
        self.parse_binary(1)
    }

    fn parse_binary(&mut self, min_precedence: u8) -> Result<Expr, Error> {
        let mut left = self.parse_unary()?;

        while let Some((op, precedence)) = self.current_binary_op() {
            if precedence < min_precedence {
                break;
            }
            self.advance();
            let right = self.parse_binary(precedence + 1)?;
            let span = Span::new(self.source, left.span().start, right.span().end);
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, Error> {
        self.enter_nesting()?;
        let result = self.parse_unary_inner();
        self.leave_nesting();
        result
    }

    fn parse_unary_inner(&mut self) -> Result<Expr, Error> {
        if self.at_arrow() {
            let start = self.current_span();
            self.advance();
            let target = self.parse_primary()?;
            let span = Span::new(self.source, start.start, target.span().end);
            return Ok(Expr::AddressOf {
                target: Box::new(target),
                span,
            });
        }
        let op = match self.kind() {
            Some(TokenKind::Minus) => Some(UnaryOp::Neg),
            Some(TokenKind::Keyword(Keyword::Not)) => Some(UnaryOp::Not),
            _ => None,
        };

        if let Some(op) = op {
            let start = self.current_span();
            self.advance();
            let operand = self.parse_unary()?;
            let span = Span::new(self.source, start.start, operand.span().end);
            return Ok(Expr::Unary {
                op,
                operand: Box::new(operand),
                span,
            });
        }

        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, Error> {
        let mut expr = self.parse_primary_base()?;
        loop {
            if matches!(self.kind(), Some(TokenKind::Dot)) {
                self.advance();
                let (field, field_span) = self.expect_identifier()?;
                let span = Span::new(self.source, expr.span().start, field_span.end);
                expr = Expr::Field {
                    base: Box::new(expr),
                    field,
                    span,
                };
            } else if self.at_arrow() {
                self.advance();
                let start = expr.span().start;
                let mode = if self.starts_mode_ref() {
                    Some(Box::new(self.parse_mode_ref()?))
                } else {
                    None
                };
                let end = mode
                    .as_ref()
                    .map(|mode| mode.span.end)
                    .unwrap_or_else(|| self.current_span().start);
                expr = Expr::Deref {
                    base: Box::new(expr),
                    mode,
                    span: Span::new(self.source, start, end),
                };
            } else {
                break;
            }
        }
        if self.at_lparen() {
            if let Expr::Field { base, field, span } = expr {
                self.advance();
                let mut args = Vec::new();
                if !self.at_rparen() {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.at_comma() {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                let end = self.expect_rparen()?;
                return Ok(Expr::MethodCall {
                    base,
                    method: field,
                    args,
                    span: Span::new(self.source, span.start, end.end),
                });
            }
        }
        Ok(expr)
    }

    fn parse_primary_base(&mut self) -> Result<Expr, Error> {
        let Some(token) = self.current() else {
            return Err(self.error_here("expected an expression"));
        };
        let span = token.span;

        match &token.kind {
            TokenKind::Integer { value, base } => {
                let expr = Expr::Integer {
                    value: *value,
                    base: *base,
                    span,
                };
                self.advance();
                Ok(expr)
            }
            TokenKind::Float(value) => {
                let expr = Expr::Float {
                    value: *value,
                    span,
                };
                self.advance();
                Ok(expr)
            }
            TokenKind::Bool(value) => {
                let expr = Expr::Bool {
                    value: *value,
                    span,
                };
                self.advance();
                Ok(expr)
            }
            TokenKind::Char(value) => {
                let expr = Expr::Char {
                    value: *value,
                    span,
                };
                self.advance();
                Ok(expr)
            }
            TokenKind::String(value) => {
                let expr = Expr::String {
                    value: value.clone(),
                    span,
                };
                self.advance();
                Ok(expr)
            }
            TokenKind::WideChar(value) => {
                let expr = Expr::WideChar {
                    value: *value,
                    span,
                };
                self.advance();
                Ok(expr)
            }
            TokenKind::WideString(value) => {
                let expr = Expr::WideString {
                    value: value.clone(),
                    span,
                };
                self.advance();
                Ok(expr)
            }
            TokenKind::Bits(value) => {
                let expr = Expr::Bits {
                    value: value.clone(),
                    span,
                };
                self.advance();
                Ok(expr)
            }
            TokenKind::Identifier(_) => {
                let (name, name_span) = self.expect_identifier()?;
                if name == "NULL" && !self.at_lparen() {
                    return Ok(Expr::Null { span });
                }
                if (name == "ALLOCATE" || name == "GETSTACK") && self.at_lparen() {
                    self.advance();
                    let mode = self.parse_mode_ref()?;
                    let value = if self.at_comma() {
                        self.advance();
                        Some(Box::new(self.parse_expr()?))
                    } else {
                        None
                    };
                    let end = self.expect_rparen()?;
                    return Ok(Expr::Allocate {
                        mode: Box::new(mode),
                        value,
                        stack: name == "GETSTACK",
                        span: Span::new(self.source, name_span.start, end.end),
                    });
                }
                if self.at_lparen() {
                    self.advance();
                    if self.at_rparen() {
                        let end = self.expect_rparen()?;
                        return Ok(Expr::Apply {
                            name,
                            args: Vec::new(),
                            span: Span::new(self.source, name_span.start, end.end),
                        });
                    }
                    let first = self.parse_expr()?;
                    if self.at_colon() {
                        self.advance();
                        let last = self.parse_expr()?;
                        let end = self.expect_rparen()?;
                        return Ok(Expr::ArraySlice {
                            name,
                            first: Box::new(first),
                            last: Box::new(last),
                            up: false,
                            span: Span::new(self.source, name_span.start, end.end),
                        });
                    }
                    if self.at_keyword(Keyword::Up) {
                        self.advance();
                        let last = self.parse_expr()?;
                        let end = self.expect_rparen()?;
                        return Ok(Expr::ArraySlice {
                            name,
                            first: Box::new(first),
                            last: Box::new(last),
                            up: true,
                            span: Span::new(self.source, name_span.start, end.end),
                        });
                    }
                    let mut args = vec![first];
                    while self.at_comma() {
                        self.advance();
                        args.push(self.parse_expr()?);
                    }
                    let end = self.expect_rparen()?;
                    Ok(Expr::Apply {
                        name,
                        args,
                        span: Span::new(self.source, name_span.start, end.end),
                    })
                } else {
                    Ok(Expr::Name {
                        name,
                        span: name_span,
                    })
                }
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect_rparen()?;
                Ok(expr)
            }
            TokenKind::LParenColon | TokenKind::LBracket => {
                let closing = if matches!(token.kind, TokenKind::LParenColon) {
                    TokenKind::RParenColon
                } else {
                    TokenKind::RBracket
                };
                self.advance();
                let mut elements = Vec::new();
                let mut labelled = None;
                if self.kind() != Some(&closing) {
                    loop {
                        let entry_start = self.current_span();
                        let labels = if self.tuple_label_follows() {
                            let labels = self.parse_case_labels()?;
                            self.expect_colon()?;
                            Some(labels)
                        } else {
                            None
                        };
                        let is_labelled = labels.is_some();
                        if let Some(expected) = labelled {
                            if expected != is_labelled {
                                return Err(self.error_here_code(
                                    "E0206",
                                    "array tuple entries must be all labelled or all unlabelled",
                                ));
                            }
                        } else {
                            labelled = Some(is_labelled);
                        }
                        let value = self.parse_expr()?;
                        let range_end = if self.at_colon() {
                            self.advance();
                            Some(Box::new(self.parse_expr()?))
                        } else {
                            None
                        };
                        let end = range_end
                            .as_ref()
                            .map(|end| end.span().end)
                            .unwrap_or(value.span().end);
                        elements.push(ArrayTupleElement {
                            labels,
                            range_end,
                            span: Span::new(self.source, entry_start.start, end),
                            value,
                        });
                        if self.at_comma() {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                let end = if self.kind() == Some(&closing) {
                    let span = self.current_span();
                    self.advance();
                    span
                } else {
                    return Err(self.error_here(format!("expected `{closing}`")));
                };
                Ok(Expr::ArrayTuple {
                    elements,
                    span: Span::new(self.source, span.start, end.end),
                })
            }
            _ => Err(self.error_here(format!("expected an expression, found {}", token.kind))),
        }
    }

    fn current_binary_op(&self) -> Option<(BinaryOp, u8)> {
        let op = match self.kind()? {
            TokenKind::Keyword(Keyword::Or) => BinaryOp::Or,
            TokenKind::Keyword(Keyword::OrIf) => BinaryOp::OrIf,
            TokenKind::Keyword(Keyword::Xor) => BinaryOp::Xor,
            TokenKind::Keyword(Keyword::And) => BinaryOp::And,
            TokenKind::Keyword(Keyword::AndIf) => BinaryOp::AndIf,
            TokenKind::Eq => BinaryOp::Eq,
            TokenKind::Ne => BinaryOp::Ne,
            TokenKind::Lt => BinaryOp::Lt,
            TokenKind::Le => BinaryOp::Le,
            TokenKind::Gt => BinaryOp::Gt,
            TokenKind::Ge => BinaryOp::Ge,
            TokenKind::Plus => BinaryOp::Add,
            TokenKind::Minus => BinaryOp::Sub,
            TokenKind::Concat => BinaryOp::Concat,
            TokenKind::Star => BinaryOp::Mul,
            TokenKind::Slash => BinaryOp::Div,
            TokenKind::Keyword(Keyword::Mod) => BinaryOp::Mod,
            TokenKind::Keyword(Keyword::Rem) => BinaryOp::Rem,
            TokenKind::Keyword(Keyword::In) => BinaryOp::In,
            TokenKind::StarStar => BinaryOp::Pow,
            _ => return None,
        };
        Some((op, op.precedence()))
    }

    fn at_end(&self) -> bool {
        self.index >= self.tokens.len()
    }

    fn current(&self) -> Option<&'a Token> {
        self.tokens.get(self.index)
    }

    fn kind(&self) -> Option<&'a TokenKind> {
        self.current().map(|token| &token.kind)
    }

    fn kind_at(&self, offset: usize) -> Option<&'a TokenKind> {
        self.tokens
            .get(self.index + offset)
            .map(|token| &token.kind)
    }

    fn current_span(&self) -> Span {
        self.current()
            .map(|token| token.span)
            .unwrap_or_else(|| self.eof_span())
    }

    fn eof_span(&self) -> Span {
        let offset = self.tokens.last().map(|token| token.span.end).unwrap_or(0);
        Span::new(self.source, offset, offset)
    }

    fn advance(&mut self) -> Option<&'a Token> {
        let token = self.current();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    fn enter_nesting(&mut self) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            self.depth -= 1;
            return Err(self.error_here_code("E0203", "nesting too deep"));
        }
        Ok(())
    }

    fn leave_nesting(&mut self) {
        self.depth -= 1;
    }

    fn error_here(&self, message: impl Into<String>) -> Error {
        Error::new(self.current_span(), message)
    }

    fn error_here_code(&self, code: &'static str, message: impl Into<String>) -> Error {
        Error::coded(code, self.current_span(), message)
    }

    fn expected(&self, what: &str) -> Error {
        let found = match self.kind() {
            Some(kind) => kind.to_string(),
            None => "end of file".to_owned(),
        };
        self.error_here(format!("expected {what}, found {found}"))
    }

    fn at_identifier(&self) -> bool {
        matches!(self.kind(), Some(TokenKind::Identifier(_)))
    }

    fn at_proc_definition(&self) -> bool {
        self.at_identifier()
            && self.kind_at(1) == Some(&TokenKind::Colon)
            && self.kind_at(2) == Some(&TokenKind::Keyword(Keyword::Proc))
    }

    fn at_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.kind(), Some(TokenKind::Keyword(found)) if *found == keyword)
    }

    fn at_comma(&self) -> bool {
        matches!(self.kind(), Some(TokenKind::Comma))
    }

    fn at_rparen(&self) -> bool {
        matches!(self.kind(), Some(TokenKind::RParen))
    }

    fn at_lparen(&self) -> bool {
        matches!(self.kind(), Some(TokenKind::LParen))
    }

    fn at_semicolon(&self) -> bool {
        matches!(self.kind(), Some(TokenKind::Semicolon))
    }

    fn at_colon(&self) -> bool {
        matches!(self.kind(), Some(TokenKind::Colon))
    }

    fn tuple_label_follows(&self) -> bool {
        if !self.at_lparen() {
            return false;
        }
        let mut depth = 0usize;
        let mut index = self.index;
        while let Some(token) = self.tokens.get(index) {
            match token.kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return matches!(
                            self.tokens.get(index + 1).map(|token| &token.kind),
                            Some(TokenKind::Colon)
                        );
                    }
                }
                _ => {}
            }
            index += 1;
        }
        false
    }

    fn starts_mode_ref(&self) -> bool {
        matches!(
            self.kind(),
            Some(
                TokenKind::Identifier(_)
                    | TokenKind::Keyword(Keyword::Array)
                    | TokenKind::Keyword(Keyword::Access)
                    | TokenKind::Keyword(Keyword::Powerset)
                    | TokenKind::Keyword(Keyword::Chars)
                    | TokenKind::Keyword(Keyword::WChars)
                    | TokenKind::Keyword(Keyword::Bools)
                    | TokenKind::Keyword(Keyword::Text)
                    | TokenKind::Keyword(Keyword::WText)
                    | TokenKind::Keyword(Keyword::Ref)
                    | TokenKind::Keyword(Keyword::Row)
            )
        )
    }

    fn at_assign(&self) -> bool {
        matches!(self.kind(), Some(TokenKind::Assign))
    }

    fn at_arrow(&self) -> bool {
        matches!(self.kind(), Some(TokenKind::Arrow))
    }

    fn expect_identifier(&mut self) -> Result<(String, Span), Error> {
        let Some(token) = self.current() else {
            return Err(self.expected("an identifier"));
        };
        match &token.kind {
            TokenKind::Identifier(name) => {
                let value = (name.clone(), token.span);
                self.advance();
                Ok(value)
            }
            _ => Err(self.expected("an identifier")),
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Result<Span, Error> {
        let Some(token) = self.current() else {
            return Err(self.expected(&format!("`{keyword}`")));
        };
        match &token.kind {
            TokenKind::Keyword(found) if *found == keyword => {
                let span = token.span;
                self.advance();
                Ok(span)
            }
            _ => Err(self.expected(&format!("`{keyword}`"))),
        }
    }

    fn expect_colon(&mut self) -> Result<Span, Error> {
        self.expect_simple(TokenKind::Colon, "`:`")
    }

    fn expect_semicolon(&mut self) -> Result<Span, Error> {
        self.expect_simple(TokenKind::Semicolon, "`;`")
    }

    fn expect_assign(&mut self) -> Result<Span, Error> {
        self.expect_simple(TokenKind::Assign, "`:=`")
    }

    fn expect_equal(&mut self) -> Result<Span, Error> {
        self.expect_simple(TokenKind::Eq, "`=`")
    }

    fn expect_arrow(&mut self) -> Result<Span, Error> {
        self.expect_simple(TokenKind::Arrow, "`->`")
    }

    fn expect_bang(&mut self) -> Result<Span, Error> {
        self.expect_simple(TokenKind::Bang, "`!`")
    }

    fn expect_lparen(&mut self) -> Result<Span, Error> {
        self.expect_simple(TokenKind::LParen, "`(`")
    }

    fn expect_rparen(&mut self) -> Result<Span, Error> {
        self.expect_simple(TokenKind::RParen, "`)`")
    }

    fn expect_simple(&mut self, kind: TokenKind, name: &str) -> Result<Span, Error> {
        let Some(token) = self.current() else {
            return Err(self.expected(name));
        };
        if token.kind == kind {
            let span = token.span;
            self.advance();
            Ok(span)
        } else {
            Err(self.expected(name))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;

    fn parse_source(source: &str) -> Program {
        let tokens = lexer::tokenize(source).unwrap();
        parse(&tokens).unwrap()
    }

    #[test]
    fn parses_hello_example() {
        let program = parse_source(include_str!("../examples/hello/src/main.ch"));
        assert_eq!(program.modules.len(), 1);

        let module = &program.modules[0];
        assert_eq!(module.name.as_deref(), Some("Hello"));
        assert_eq!(module.declarations.len(), 2);
        assert_eq!(module.actions.len(), 2);

        let Declaration::Proc(proc_def) = &module.declarations[0] else {
            panic!("expected a procedure");
        };
        assert_eq!(proc_def.name, "AddScaled");
        assert_eq!(proc_def.params.len(), 2);
        assert_eq!(proc_def.params[0].name, "a");
        assert_eq!(proc_def.params[0].mode.name, "SHORT_INT");
        assert_eq!(proc_def.params[1].name, "b");
        assert_eq!(
            proc_def.result.as_ref().map(|mode| mode.name.as_str()),
            Some("LONG_INT")
        );
        assert_eq!(proc_def.declarations.len(), 2);
        assert_eq!(proc_def.actions.len(), 3);

        assert!(matches!(
            proc_def.actions[2],
            Action::Return { value: Some(_), .. }
        ));
        assert!(matches!(module.actions[1], Action::Assert { .. }));
    }

    #[test]
    fn arithmetic_precedence() {
        let program = parse_source(
            r#"
M: MODULE
  DCL x INT;
  x := a + b * c = d;
END M;
"#,
        );

        let Action::Assign { value, .. } = &program.modules[0].actions[0] else {
            panic!("expected assignment");
        };
        let Expr::Binary {
            op: BinaryOp::Eq,
            left,
            ..
        } = value
        else {
            panic!("expected equality at the root");
        };
        let Expr::Binary {
            op: BinaryOp::Add,
            right,
            ..
        } = left.as_ref()
        else {
            panic!("expected addition on the left");
        };
        assert!(matches!(
            right.as_ref(),
            Expr::Binary {
                op: BinaryOp::Mul,
                ..
            }
        ));
    }

    #[test]
    fn mode_conversion_parses_as_application() {
        let program = parse_source(
            r#"
M: MODULE
  DCL y LONG_INT;
  y := LONG_INT(x) * 2;
END M;
"#,
        );

        let Action::Assign { value, .. } = &program.modules[0].actions[0] else {
            panic!("expected assignment");
        };
        let Expr::Binary {
            op: BinaryOp::Mul,
            left,
            ..
        } = value
        else {
            panic!("expected multiplication");
        };
        assert!(matches!(
            left.as_ref(),
            Expr::Apply { name, args, .. } if name == "LONG_INT" && args.len() == 1
        ));
    }

    #[test]
    fn parses_parameter_name_lists() {
        let program = parse_source(
            r#"
M: MODULE
  P: PROC (a, b INT IN, c LONG_INT OUT) RETURNS (INT);
    RETURN (a);
  END P;
END M;
"#,
        );

        let Declaration::Proc(proc_def) = &program.modules[0].declarations[0] else {
            panic!("expected procedure");
        };
        assert_eq!(proc_def.params.len(), 3);
        assert_eq!(proc_def.params[0].name, "a");
        assert_eq!(proc_def.params[1].name, "b");
        assert_eq!(proc_def.params[1].passing, Passing::In);
        assert_eq!(proc_def.params[2].name, "c");
        assert_eq!(proc_def.params[2].passing, Passing::Out);
    }

    #[test]
    fn parses_visibility_statements() {
        let program = parse_source(
            r#"
M: MODULE
  GRANT x, F;
  SEIZE y;
END M;
"#,
        );

        let declarations = &program.modules[0].declarations;
        assert_eq!(declarations.len(), 2);

        let Declaration::Visibility(grant) = &declarations[0] else {
            panic!("expected GRANT");
        };
        assert_eq!(grant.kind, VisibilityKind::Grant);
        assert_eq!(grant.names.len(), 2);
        assert_eq!(grant.names[0].name, "x");
        assert_eq!(grant.names[1].name, "F");

        let Declaration::Visibility(seize) = &declarations[1] else {
            panic!("expected SEIZE");
        };
        assert_eq!(seize.kind, VisibilityKind::Seize);
        assert_eq!(seize.names[0].name, "y");
    }

    #[test]
    fn visibility_is_rejected_inside_procedures() {
        let tokens = lexer::tokenize(
            r#"
M: MODULE
  P: PROC ();
    GRANT x;
  END P;
END M;
"#,
        )
        .unwrap();
        let error = parse(&tokens).unwrap_err();
        assert!(error.message.contains("only allowed at module level"));
    }

    #[test]
    fn parses_spec_module_and_body() {
        let program = parse_source(
            r#"
M: SPEC MODULE
  GRANT F;
  F: PROC (a INT) RETURNS (INT) END;
END M;

M: MODULE
  F: PROC (a INT) RETURNS (INT);
    RETURN (a);
  END F;
END M;
"#,
        );

        assert_eq!(program.modules.len(), 2);
        assert_eq!(program.modules[0].kind, ModuleKind::Spec);
        assert!(matches!(
            program.modules[0].declarations[1],
            Declaration::ProcSignature(_)
        ));
        assert_eq!(program.modules[1].kind, ModuleKind::Body);
    }

    #[test]
    fn spec_modules_reject_actions() {
        let tokens = lexer::tokenize(
            r#"
M: SPEC MODULE
  F: PROC () END;
  ASSERT TRUE;
END M;
"#,
        )
        .unwrap();
        let error = parse(&tokens).unwrap_err();
        assert!(error.message.contains("cannot contain actions"));
    }

    #[test]
    fn parses_do_for_with_step_and_down() {
        let program = parse_source(
            r#"
M: MODULE
  DO FOR i := 10 BY 2 DOWN TO 1;
  OD;
END M;
"#,
        );

        let Action::Do {
            control:
                DoControl::For {
                    counter,
                    step,
                    down,
                    ..
                },
            ..
        } = &program.modules[0].actions[0]
        else {
            panic!("expected DO FOR");
        };

        assert_eq!(counter, "i");
        assert!(step.is_some());
        assert!(*down);
    }

    #[test]
    fn parses_case_labels() {
        let program = parse_source(
            r#"
M: MODULE
  CASE x OF
    (1, 2): ;
    (3:5): ;
    (ELSE): ;
  ESAC;
END M;
"#,
        );

        let Action::Case { alternatives, .. } = &program.modules[0].actions[0] else {
            panic!("expected CASE");
        };

        assert_eq!(alternatives.len(), 3);
        assert_eq!(
            alternatives[0].labels,
            vec![CaseLabel::Value(1), CaseLabel::Value(2)]
        );
        assert_eq!(
            alternatives[1].labels,
            vec![CaseLabel::Range { low: 3, high: 5 }]
        );
        assert_eq!(alternatives[2].labels, vec![CaseLabel::Else]);
    }

    #[test]
    fn parses_labelled_action_and_exit() {
        let program = parse_source(
            r#"
M: MODULE
L: DO
    EXIT L;
  OD;
END M;
"#,
        );

        let Action::Labelled { label, action, .. } = &program.modules[0].actions[0] else {
            panic!("expected labelled action");
        };
        assert_eq!(label, "L");

        let Action::Do { body, .. } = action.as_ref() else {
            panic!("expected DO");
        };
        assert!(matches!(
            &body[0],
            Action::Exit { label: Some(name), .. } if name == "L"
        ));
    }

    #[test]
    fn rejects_excessive_nesting() {
        let source = format!(
            "M: MODULE\n  ASSERT {}1{} = 1;\nEND M;\n",
            "(".repeat(MAX_NESTING_DEPTH + 10),
            ")".repeat(MAX_NESTING_DEPTH + 10)
        );
        let tokens = lexer::tokenize(&source).unwrap();
        let error = parse(&tokens).unwrap_err();
        assert_eq!(error.code, "E0203");
        assert!(error.message.contains("nesting too deep"));
    }

    #[test]
    fn parses_qualified_visibility() {
        let program = parse_source(
            r#"
M: MODULE
  GRANT ALL TO pkg!Other;
  SEIZE A!x;
END M;
"#,
        );

        let Declaration::Visibility(grant) = &program.modules[0].declarations[0] else {
            panic!("expected GRANT");
        };
        assert!(grant.all);
        assert!(grant.names.is_empty());
        assert_eq!(grant.friends.len(), 1);
        assert_eq!(grant.friends[0].qualified(), "pkg!Other");

        let Declaration::Visibility(seize) = &program.modules[0].declarations[1] else {
            panic!("expected SEIZE");
        };
        assert_eq!(seize.names[0].qualified(), "A!x");
        assert_eq!(seize.names[0].prefix, vec!["A".to_owned()]);
        assert_eq!(seize.names[0].name, "x");
    }

    #[test]
    fn parses_grant_prefixes() {
        let program = parse_source(
            r#"
M: MODULE
  GRANT x PREFIXED P;
  GRANT ALL PREFIXED;
  GRANT ( -> Q ) ! y;
END M;
"#,
        );

        let Declaration::Visibility(prefix) = &program.modules[0].declarations[0] else {
            panic!("expected GRANT");
        };
        assert_eq!(
            prefix.prefix,
            Some(PrefixClause::Prefixed(Some(vec!["P".to_owned()])))
        );

        let Declaration::Visibility(module_prefix) = &program.modules[0].declarations[1] else {
            panic!("expected GRANT ALL");
        };
        assert!(module_prefix.all);
        assert_eq!(module_prefix.prefix, Some(PrefixClause::Prefixed(None)));

        let Declaration::Visibility(rename) = &program.modules[0].declarations[2] else {
            panic!("expected prefix rename");
        };
        assert_eq!(
            rename.prefix,
            Some(PrefixClause::Rename {
                old: Vec::new(),
                new: vec!["Q".to_owned()],
            })
        );
        assert_eq!(rename.names[0].name, "y");
    }

    #[test]
    fn parses_seize_prefixes() {
        let program = parse_source(
            r#"
M: MODULE
  SEIZE x PREFIXED P;
  SEIZE ( -> Q ) ! y;
  SEIZE (R -> ) ! z;
END M;
"#,
        );

        let Declaration::Visibility(prefixed) = &program.modules[0].declarations[0] else {
            panic!("expected SEIZE");
        };
        assert_eq!(
            prefixed.prefix,
            Some(PrefixClause::Prefixed(Some(vec!["P".to_owned()])))
        );

        let Declaration::Visibility(new_prefix) = &program.modules[0].declarations[1] else {
            panic!("expected SEIZE rename");
        };
        assert_eq!(
            new_prefix.prefix,
            Some(PrefixClause::Rename {
                old: Vec::new(),
                new: vec!["Q".to_owned()],
            })
        );

        let Declaration::Visibility(old_prefix) = &program.modules[0].declarations[2] else {
            panic!("expected SEIZE rename");
        };
        assert_eq!(
            old_prefix.prefix,
            Some(PrefixClause::Rename {
                old: vec!["R".to_owned()],
                new: Vec::new(),
            })
        );
    }

    #[test]
    fn parses_context_list() {
        let program = parse_source(
            r#"
CONTEXT
  DCL x INT;
  F: PROC (a INT) RETURNS (INT) END;
FOR
M: MODULE
  SEIZE x, F;
END M;
"#,
        );

        assert_eq!(program.modules.len(), 1);
        let module = &program.modules[0];
        assert_eq!(module.contexts.len(), 1);
        assert_eq!(module.contexts[0].declarations.len(), 2);
    }

    #[test]
    fn parses_regions_and_spec_regions() {
        let program = parse_source(
            r#"
R: REGION
  DCL x INT;
END R;

S: SPEC REGION
  GRANT x;
  DCL x INT;
END S;

T: REGION SPEC
  DCL y INT;
END T;
"#,
        );

        assert_eq!(program.modules[0].kind, ModuleKind::Region);
        assert_eq!(program.modules[1].kind, ModuleKind::SpecRegion);
        assert_eq!(program.modules[2].kind, ModuleKind::SpecRegion);
    }

    #[test]
    fn regions_reject_actions() {
        let tokens = lexer::tokenize(
            r#"
R: REGION
  ASSERT TRUE;
END R;
"#,
        )
        .unwrap();
        let error = parse(&tokens).unwrap_err();
        assert!(error.message.contains("region cannot contain actions"));
    }

    #[test]
    fn parses_nested_spec_modules() {
        let program = parse_source(
            r#"
Outer: SPEC MODULE
  Inner: SPEC MODULE
    DCL x INT;
    GRANT x;
  END Inner;
END Outer;
"#,
        );

        let Declaration::SpecModule(inner) = &program.modules[0].declarations[0] else {
            panic!("expected nested spec module");
        };
        assert_eq!(inner.name.as_deref(), Some("Inner"));
        assert_eq!(inner.kind, ModuleKind::Spec);
        assert_eq!(inner.declarations.len(), 2);
    }

    #[test]
    fn parses_remote_units_and_contexts() {
        let program = parse_source(
            r#"
REMOTE "pieces/a.ch";
M: REMOTE counter;
S: SPEC REMOTE "pieces/s.ch";
SPEC REMOTE;
CONTEXT MODULE REMOTE "pieces/m.ch";
CONTEXT REMOTE "pieces/c.ch"
  DCL x INT;
FOR
C: MODULE
END C;
"#,
        );

        assert_eq!(program.remotes.len(), 5);
        assert_eq!(program.remotes[0].kind, RemoteKind::ProgramUnit);
        assert!(matches!(
            program.remotes[0].designator.kind,
            PieceDesignatorKind::Literal(ref path) if path == "pieces/a.ch"
        ));
        assert_eq!(program.remotes[1].kind, RemoteKind::Module);
        assert_eq!(program.remotes[1].name.as_deref(), Some("M"));
        assert!(matches!(
            program.remotes[1].designator.kind,
            PieceDesignatorKind::TextRef(ref name) if name == "counter"
        ));
        assert_eq!(program.remotes[2].kind, RemoteKind::SpecModule);
        assert!(matches!(
            program.remotes[3].designator.kind,
            PieceDesignatorKind::Empty
        ));
        assert_eq!(program.remotes[4].kind, RemoteKind::ContextModule);

        assert_eq!(program.modules.len(), 1);
        let context = &program.modules[0].contexts[0];
        assert!(context.remote.is_some());
        assert_eq!(context.declarations.len(), 1);
    }

    #[test]
    fn parses_context_visibility_in_later_contexts() {
        let program = parse_source(
            r#"
CONTEXT
  DCL x INT;
FOR
CONTEXT
  SEIZE x;
  GRANT x;
FOR
M: MODULE
END M;
"#,
        );
        let module = &program.modules[0];
        assert_eq!(module.contexts.len(), 2);
        assert_eq!(module.contexts[1].declarations.len(), 2);
    }

    #[test]
    fn context_rejects_initializers_and_visibility() {
        let tokens = lexer::tokenize(
            r#"
CONTEXT
  DCL x INT INIT := 1;
FOR
M: MODULE
END M;
"#,
        )
        .unwrap();
        let error = parse(&tokens).unwrap_err();
        assert!(error.message.contains("initialization"));

        let tokens = lexer::tokenize(
            r#"
CONTEXT
  GRANT x;
FOR
M: MODULE
END M;
"#,
        )
        .unwrap();
        let error = parse(&tokens).unwrap_err();
        assert!(error.message.contains("context"));
    }

    #[test]
    fn parses_nested_modules_with_contexts() {
        let program = parse_source(
            r#"
M: MODULE
  DCL x INT;
  CONTEXT
    DCL x INT;
  FOR
  Helper: MODULE
    SEIZE x;
  END Helper;
  MODULE
  END;
END M;
"#,
        );

        let Action::Module(helper) = &program.modules[0].actions[0] else {
            panic!("expected nested module");
        };
        assert_eq!(helper.name.as_deref(), Some("Helper"));
        assert_eq!(helper.contexts.len(), 1);
        assert_eq!(helper.declarations.len(), 1);

        assert!(matches!(
            &program.modules[0].actions[1],
            Action::Module(module) if module.name.is_none()
        ));
    }

    #[test]
    fn parses_structure_mode_and_forbid() {
        let program = parse_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT, hidden BOOL);
  GRANT S FORBID (hidden);
END M;
"#,
        );

        let Declaration::Mode(mode) = &program.modules[0].declarations[0] else {
            panic!("expected NEWMODE");
        };
        assert_eq!(mode.name, "S");
        let ModeDefinitionBody::Struct(fields) = &mode.body else {
            panic!("expected STRUCT");
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "x");
        assert_eq!(fields[1].name, "hidden");

        let Declaration::Visibility(grant) = &program.modules[0].declarations[1] else {
            panic!("expected GRANT");
        };
        let forbid = grant.forbid.as_ref().unwrap();
        assert_eq!(forbid.fields, vec!["hidden".to_owned()]);
        assert!(!forbid.all);
    }

    #[test]
    fn missing_semicolon_is_an_error() {
        let tokens = lexer::tokenize("M: MODULE DCL x INT END M;").unwrap();
        let error = parse(&tokens).unwrap_err();
        assert!(error.message.contains("expected `;`"));
    }
}

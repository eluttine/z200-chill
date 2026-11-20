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

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
    source: SourceId,
    depth: usize,
}

const MAX_NESTING_DEPTH: usize = 256;

impl<'a> Parser<'a> {
    fn parse_program(&mut self) -> Result<Program, Error> {
        let mut modules = Vec::new();
        while !self.at_end() {
            modules.push(self.parse_module()?);
        }
        if modules.is_empty() {
            return Err(self.error_here("expected a module"));
        }
        Ok(Program { modules })
    }

    fn parse_module(&mut self) -> Result<Module, Error> {
        let start = self.current_span();

        let name = if self.at_identifier() && self.kind_at(1) == Some(&TokenKind::Colon) {
            let (name, _) = self.expect_identifier()?;
            self.expect_colon()?;
            Some(name)
        } else {
            None
        };

        let kind = if self.at_keyword(Keyword::Spec) {
            self.advance();
            self.expect_keyword(Keyword::Module)?;
            ModuleKind::Spec
        } else {
            self.expect_keyword(Keyword::Module)?;
            if self.at_keyword(Keyword::Spec) {
                self.advance();
                ModuleKind::Spec
            } else {
                if self.at_keyword(Keyword::Body) {
                    self.advance();
                }
                ModuleKind::Body
            }
        };

        if kind == ModuleKind::Spec && name.is_none() {
            return Err(self.error_here_code("E0204", "spec module must be named"));
        }

        let declarations = self.parse_declarations(true, kind == ModuleKind::Spec)?;
        let actions = if kind == ModuleKind::Spec {
            if !self.at_keyword(Keyword::End) {
                return Err(self.error_here_code("E0204", "spec module cannot contain actions"));
            }
            Vec::new()
        } else {
            self.parse_actions_until(&[Keyword::End])?
        };
        self.expect_keyword(Keyword::End)?;

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
            declarations,
            actions,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_declarations(
        &mut self,
        allow_visibility: bool,
        spec: bool,
    ) -> Result<Vec<Declaration>, Error> {
        let mut declarations = Vec::new();
        loop {
            if self.at_keyword(Keyword::Dcl) {
                declarations.push(Declaration::Var(self.parse_dcl()?));
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
            if kind == VisibilityKind::Seize {
                return Err(self.error_here_code(
                    "E0205",
                    "SEIZE prefix-rename clauses are not implemented yet",
                ));
            }
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

            if !old.is_empty() {
                return Err(self.error_here_code(
                    "E0205",
                    "prefix rename with a non-empty old prefix is not implemented yet",
                ));
            }
            prefix = Some(new);
        }

        let mut names = Vec::new();
        let mut all = false;
        let mut friends = Vec::new();

        if self.at_keyword(Keyword::All) {
            self.advance();
            all = true;
        } else {
            loop {
                if self.at_keyword(Keyword::Forbid) {
                    return Err(self.error_here_code(
                        "E0205",
                        "FORBID is not supported yet (structure modes are not implemented)",
                    ));
                }
                names.push(self.parse_name_ref()?);
                if self.at_comma() {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        if self.at_keyword(Keyword::Prefixed) {
            if kind == VisibilityKind::Seize {
                return Err(self.error_here_code("E0205", "SEIZE PREFIXED is not implemented yet"));
            }
            self.advance();
            let parts = if self.at_identifier() {
                self.parse_prefix_parts()?
            } else {
                Vec::new()
            };
            prefix = Some(parts);
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

        if self.at_keyword(Keyword::Forbid) {
            return Err(self.error_here_code(
                "E0205",
                "FORBID is not supported yet (structure modes are not implemented)",
            ));
        }

        let end = self.expect_semicolon()?;
        Ok(Visibility {
            kind,
            names,
            all,
            friends,
            prefix,
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
        let (name, span) = self.expect_identifier()?;
        Ok(ModeRef { name, span })
    }

    fn parse_actions_until(&mut self, stop: &[Keyword]) -> Result<Vec<Action>, Error> {
        let mut actions = Vec::new();
        while !self.at_end() && !stop.iter().any(|keyword| self.at_keyword(*keyword)) {
            actions.push(self.parse_action()?);
        }
        Ok(actions)
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
            let action = self.parse_action()?;
            return Ok(Action::Labelled {
                label,
                span: Span::new(self.source, start.start, action.span().end),
                action: Box::new(action),
            });
        }

        match self.kind() {
            Some(TokenKind::Keyword(Keyword::Assert)) => self.parse_assert(),
            Some(TokenKind::Keyword(Keyword::Return)) => self.parse_return(),
            Some(TokenKind::Keyword(Keyword::Result)) => self.parse_result(),
            Some(TokenKind::Keyword(Keyword::Exit)) => self.parse_exit(),
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

    fn parse_assert(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Assert)?;
        let condition = self.parse_expr()?;
        let end = self.expect_semicolon()?;
        Ok(Action::Assert {
            condition,
            span: Span::new(self.source, start.start, end.end),
        })
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
        self.expect_keyword(Keyword::Then)?;
        let body = self.parse_actions_until(&[Keyword::Elsif, Keyword::Else, Keyword::Fi])?;
        let mut branches = vec![Branch { condition, body }];

        while self.at_keyword(Keyword::Elsif) {
            self.advance();
            let condition = self.parse_expr()?;
            self.expect_keyword(Keyword::Then)?;
            let body = self.parse_actions_until(&[Keyword::Elsif, Keyword::Else, Keyword::Fi])?;
            branches.push(Branch { condition, body });
        }

        let else_actions = if self.at_keyword(Keyword::Else) {
            self.advance();
            Some(self.parse_actions_until(&[Keyword::Fi])?)
        } else {
            None
        };

        self.expect_keyword(Keyword::Fi)?;
        let end = self.expect_semicolon()?;
        Ok(Action::If {
            branches,
            else_actions,
            span: Span::new(self.source, start.start, end.end),
        })
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

        let else_actions = if self.at_keyword(Keyword::Else) {
            self.advance();
            Some(self.parse_actions_until(&[Keyword::Esac])?)
        } else {
            None
        };

        self.expect_keyword(Keyword::Esac)?;
        let end = self.expect_semicolon()?;

        if alternatives.is_empty() && else_actions.is_none() {
            return Err(Error::coded(
                "E0206",
                start,
                "CASE requires at least one alternative",
            ));
        }

        Ok(Action::Case {
            selector,
            alternatives,
            else_actions,
            span: Span::new(self.source, start.start, end.end),
        })
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
        let end = self.expect_semicolon()?;

        Ok(Action::Do {
            control,
            body,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_begin(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Begin)?;
        let declarations = self.parse_declarations(false, false)?;
        let actions = self.parse_actions_until(&[Keyword::End])?;
        self.expect_keyword(Keyword::End)?;
        let end = self.expect_semicolon()?;

        Ok(Action::Begin {
            declarations,
            actions,
            span: Span::new(self.source, start.start, end.end),
        })
    }

    fn parse_expression_action(&mut self) -> Result<Action, Error> {
        let target = self.parse_expr()?;

        if let Some(op) = self.assignment_operator() {
            let start = target.span().start;
            self.advance();
            let value = self.parse_expr()?;
            let value_end = value.span().end;
            self.expect_semicolon()?;
            return Ok(Action::Assign {
                target,
                op,
                value,
                span: Span::new(self.source, start, value_end),
            });
        }

        let span = target.span();
        let callee = match target {
            Expr::Apply { .. } => target,
            _ => return Err(Error::new(span, "expected assignment or procedure call")),
        };
        self.expect_semicolon()?;
        Ok(Action::Call { callee, span })
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
            TokenKind::Identifier(_) => {
                let (name, name_span) = self.expect_identifier()?;
                if self.at_lparen() {
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
        assert!(error.message.contains("spec module cannot contain actions"));
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
        assert_eq!(prefix.prefix, Some(vec!["P".to_owned()]));

        let Declaration::Visibility(module_prefix) = &program.modules[0].declarations[1] else {
            panic!("expected GRANT ALL");
        };
        assert!(module_prefix.all);
        assert_eq!(module_prefix.prefix, Some(Vec::new()));

        let Declaration::Visibility(rename) = &program.modules[0].declarations[2] else {
            panic!("expected prefix rename");
        };
        assert_eq!(rename.prefix, Some(vec!["Q".to_owned()]));
        assert_eq!(rename.names[0].name, "y");
    }

    #[test]
    fn missing_semicolon_is_an_error() {
        let tokens = lexer::tokenize("M: MODULE DCL x INT END M;").unwrap();
        let error = parse(&tokens).unwrap_err();
        assert!(error.message.contains("expected `;`"));
    }
}

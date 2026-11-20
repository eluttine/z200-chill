use crate::ast::*;
use crate::lexer::{Keyword, Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub span: Span,
    pub message: String,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
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
    Parser { tokens, index: 0 }.parse_program()
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
}

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

        self.expect_keyword(Keyword::Module)?;
        if self.at_keyword(Keyword::Body) {
            self.advance();
        }

        let declarations = self.parse_declarations(true)?;
        let actions = self.parse_actions_until(&[Keyword::End])?;
        self.expect_keyword(Keyword::End)?;

        if let Some(TokenKind::Identifier(trailing)) = self.kind() {
            if let Some(expected) = &name {
                if trailing != expected {
                    return Err(self.error_here(format!(
                        "module end name `{trailing}` does not match `{expected}`"
                    )));
                }
            }
            self.advance();
        }

        let end = self.expect_semicolon()?;
        Ok(Module {
            name,
            declarations,
            actions,
            span: Span {
                start: start.start,
                end: end.end,
            },
        })
    }

    fn parse_declarations(&mut self, allow_visibility: bool) -> Result<Vec<Declaration>, Error> {
        let mut declarations = Vec::new();
        loop {
            if self.at_keyword(Keyword::Dcl) {
                declarations.push(Declaration::Var(self.parse_dcl()?));
            } else if self.at_proc_definition() {
                declarations.push(Declaration::Proc(self.parse_proc()?));
            } else if self.at_keyword(Keyword::Grant) {
                if !allow_visibility {
                    return Err(self.error_here("GRANT is only allowed at module level"));
                }
                declarations.push(Declaration::Visibility(
                    self.parse_visibility(VisibilityKind::Grant)?,
                ));
            } else if self.at_keyword(Keyword::Seize) {
                if !allow_visibility {
                    return Err(self.error_here("SEIZE is only allowed at module level"));
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

        let mut names = Vec::new();
        loop {
            if matches!(self.kind(), Some(TokenKind::Keyword(Keyword::All))) {
                return Err(self.error_here("GRANT/SEIZE ALL is not implemented yet"));
            }
            let (name, span) = self.expect_identifier()?;
            names.push(NameRef { name, span });
            if self.at_comma() {
                self.advance();
            } else {
                break;
            }
        }

        let end = self.expect_semicolon()?;
        Ok(Visibility {
            kind,
            names,
            span: Span {
                start: start.start,
                end: end.end,
            },
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
                    span: Span {
                        start: name_span.start,
                        end,
                    },
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
                        span: Span {
                            start: name_span.start,
                            end: param_end,
                        },
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

        self.expect_semicolon()?;
        let declarations = self.parse_declarations(false)?;
        let actions = self.parse_actions_until(&[Keyword::End])?;
        self.expect_keyword(Keyword::End)?;

        if let Some(TokenKind::Identifier(trailing)) = self.kind() {
            if trailing != &name {
                return Err(self.error_here(format!(
                    "procedure end name `{trailing}` does not match `{name}`"
                )));
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
            span: Span {
                start: start.start,
                end: end.end,
            },
        })
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
        match self.kind() {
            Some(TokenKind::Keyword(Keyword::Assert)) => self.parse_assert(),
            Some(TokenKind::Keyword(Keyword::Return)) => self.parse_return(),
            Some(TokenKind::Keyword(Keyword::Result)) => self.parse_result(),
            Some(TokenKind::Keyword(Keyword::Exit)) => self.parse_exit(),
            Some(TokenKind::Keyword(Keyword::If)) => self.parse_if(),
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
            span: Span {
                start: start.start,
                end: end.end,
            },
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
            span: Span {
                start: start.start,
                end: end.end,
            },
        })
    }

    fn parse_result(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Result)?;
        let value = self.parse_expr()?;
        let end = self.expect_semicolon()?;
        Ok(Action::Result {
            value,
            span: Span {
                start: start.start,
                end: end.end,
            },
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
            span: Span {
                start: start.start,
                end: end.end,
            },
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
            span: Span {
                start: start.start,
                end: end.end,
            },
        })
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
                self.expect_keyword(Keyword::To)?;
                let end_value = self.parse_expr()?;
                let step = if self.at_keyword(Keyword::By) {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                DoControl::For {
                    counter,
                    start: start_value,
                    end: end_value,
                    step,
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
            span: Span {
                start: start.start,
                end: end.end,
            },
        })
    }

    fn parse_begin(&mut self) -> Result<Action, Error> {
        let start = self.current_span();
        self.expect_keyword(Keyword::Begin)?;
        let declarations = self.parse_declarations(false)?;
        let actions = self.parse_actions_until(&[Keyword::End])?;
        self.expect_keyword(Keyword::End)?;
        let end = self.expect_semicolon()?;

        Ok(Action::Begin {
            declarations,
            actions,
            span: Span {
                start: start.start,
                end: end.end,
            },
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
                span: Span {
                    start,
                    end: value_end,
                },
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
            let span = Span {
                start: left.span().start,
                end: right.span().end,
            };
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
        let op = match self.kind() {
            Some(TokenKind::Minus) => Some(UnaryOp::Neg),
            Some(TokenKind::Keyword(Keyword::Not)) => Some(UnaryOp::Not),
            _ => None,
        };

        if let Some(op) = op {
            let start = self.current_span();
            self.advance();
            let operand = self.parse_unary()?;
            let span = Span {
                start: start.start,
                end: operand.span().end,
            };
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
                        span: Span {
                            start: name_span.start,
                            end: end.end,
                        },
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
        Span {
            start: offset,
            end: offset,
        }
    }

    fn advance(&mut self) -> Option<&'a Token> {
        let token = self.current();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    fn error_here(&self, message: impl Into<String>) -> Error {
        Error::new(self.current_span(), message)
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
    fn missing_semicolon_is_an_error() {
        let tokens = lexer::tokenize("M: MODULE DCL x INT END M;").unwrap();
        let error = parse(&tokens).unwrap_err();
        assert!(error.message.contains("expected `;`"));
    }
}

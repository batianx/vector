use std::fmt;

use diagnostic::{DiagnosticMessage, Label, Note, Urls};
use parser::ast::Node;
use value::Value;

use super::Expr;
use crate::{
    expression::{ExpressionError, Resolved},
    state::{ExternalEnv, LocalEnv},
    value::Kind,
    value::VrlValueConvert,
    BatchContext, Context, Expression, Span, TypeDef,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Abort {
    span: Span,
    message: Option<Box<Expr>>,
}

impl Abort {
    pub fn new(
        span: Span,
        message: Option<Node<Expr>>,
        state: (&LocalEnv, &ExternalEnv),
    ) -> Result<Self, Error> {
        let message = message
            .map(|node| {
                let (expr_span, expr) = node.take();
                let type_def = expr.type_def(state);

                if type_def.is_fallible() {
                    Err(Error {
                        variant: ErrorVariant::FallibleExpr,
                        expr_span,
                    })
                } else if !type_def.is_bytes() {
                    Err(Error {
                        variant: ErrorVariant::NonString(type_def.into()),
                        expr_span,
                    })
                } else {
                    Ok(Box::new(expr))
                }
            })
            .transpose()?;

        Ok(Self { span, message })
    }
}

impl Expression for Abort {
    fn resolve(&self, ctx: &mut Context) -> Resolved {
        let message = self
            .message
            .as_ref()
            .map::<Result<_, ExpressionError>, _>(|expr| {
                Ok(expr.resolve(ctx)?.try_bytes_utf8_lossy()?.to_string())
            })
            .transpose()?;

        Err(ExpressionError::Abort {
            span: self.span,
            message,
        })
    }

    fn resolve_batch(&self, ctx: &mut BatchContext) {
        let messages: Vec<_> = if let Some(expr) = &self.message {
            expr.resolve_batch(ctx);
            ctx.resolved_values_mut()
                .iter_mut()
                .map(|resolved| {
                    let resolved = {
                        let mut moved = Ok(Value::Null);
                        std::mem::swap(resolved, &mut moved);
                        moved
                    };
                    (|| -> Result<_, ExpressionError> {
                        Ok(Some(resolved?.try_bytes_utf8_lossy()?.to_string()))
                    })()
                })
                .collect()
        } else {
            ctx.resolved_values_mut().iter().map(|_| Ok(None)).collect()
        };

        for (resolved, message) in ctx.resolved_values_mut().iter_mut().zip(messages) {
            *resolved = message.and_then(|message| {
                Err(ExpressionError::Abort {
                    span: self.span,
                    message,
                })
            });
        }
    }

    fn type_def(&self, _: (&LocalEnv, &ExternalEnv)) -> TypeDef {
        TypeDef::never().infallible()
    }
}

impl fmt::Display for Abort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "abort")
    }
}

// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct Error {
    variant: ErrorVariant,
    expr_span: Span,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum ErrorVariant {
    #[error("unhandled fallible expression")]
    FallibleExpr,
    #[error("non-string abort message")]
    NonString(Kind),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#}", self.variant)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.variant)
    }
}

impl DiagnosticMessage for Error {
    fn code(&self) -> usize {
        use ErrorVariant::*;

        match self.variant {
            FallibleExpr => 631,
            NonString(_) => 300,
        }
    }

    fn labels(&self) -> Vec<Label> {
        match &self.variant {
            ErrorVariant::FallibleExpr => vec![
                Label::primary(
                    "abort only accepts an infallible expression argument",
                    self.expr_span,
                ),
                Label::context(
                    "handle errors before using the expression as an abort message",
                    self.expr_span,
                ),
            ],
            ErrorVariant::NonString(kind) => vec![
                Label::primary(
                    "abort only accepts an expression argument resolving to a string",
                    self.expr_span,
                ),
                Label::context(
                    format!("this expression resolves to {}", kind),
                    self.expr_span,
                ),
            ],
        }
    }

    fn notes(&self) -> Vec<Note> {
        match self.variant {
            ErrorVariant::FallibleExpr => vec![Note::SeeErrorDocs],
            ErrorVariant::NonString(_) => vec![
                Note::CoerceValue,
                Note::SeeDocs(
                    "type coercion".to_owned(),
                    Urls::func_docs("#coerce-functions"),
                ),
            ],
        }
    }
}

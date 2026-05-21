use std::borrow::Cow;

use oxc_ast::ast::Expression;
use oxc_formatter_core::{
    Buffer, Format, Formatter,
    builders::{FormatWith, text},
    write,
};
use oxc_span::{GetSpan, Span};

use crate::{
    comments::{FormatLeadingComments, is_suppressed_before, write_suppressed_node},
    context::JsonFormatContext,
};

pub mod array;
pub mod literal;
pub mod object;

pub type JsonFormatter<'buf, 'a> = Formatter<'buf, 'a, JsonFormatContext<'a>>;

/// `Format` impl for `&'static str` specialized to `JsonFormatContext`.
///
/// Hardcoded to `JsonFormatContext` rather than generic over `C` so the blanket
/// `&T where T: Format` doesn't overlap (`str` doesn't impl `Format` for any C).
impl<'a> Format<'a, JsonFormatContext<'a>> for &'static str {
    #[inline]
    fn fmt(&self, f: &mut JsonFormatter<'_, 'a>) {
        write!(f, oxc_formatter_core::builders::token(self));
    }
}

/// Write the verbatim source slice covered by `span`.
///
/// The slice borrows directly from the arena-resident source (`JsonFormatContext::source`),
/// so no `alloc_str` is needed.
pub fn write_source_slice(span: Span, f: &mut JsonFormatter<'_, '_>) {
    let s = &f.context().source()[span.start as usize..span.end as usize];
    write!(f, text(s));
}

/// Lifts a `Cow<'a, str>` to `&'a str`, allocating in the arena only for the
/// owned case. Borrowed Cows already point into arena-resident source, so they
/// pass through unchanged.
pub fn arena_cow_str<'a>(cow: Cow<'a, str>, f: &JsonFormatter<'_, 'a>) -> &'a str {
    match cow {
        Cow::Borrowed(s) => s,
        Cow::Owned(s) => f.allocator().alloc_str(&s),
    }
}

/// Wraps a re-entrant JSON closure in a [`FormatWith`]. The closure's context is
/// pinned to [`JsonFormatContext`] so call sites don't have to annotate it.
#[inline]
pub const fn format_with<'a, T>(formatter: T) -> FormatWith<T>
where
    T: Fn(&mut JsonFormatter<'_, 'a>),
{
    FormatWith::new(formatter)
}

/// Top-level wrapper around an [`Expression`]. Dispatches by variant.
///
/// Drains any leading comments that precede `expression.span.start` and emits them before
/// the value itself.
pub struct FmtJsonValue<'a, 'b> {
    pub expression: &'b Expression<'a>,
}

impl<'a> Format<'a, JsonFormatContext<'a>> for FmtJsonValue<'a, '_> {
    fn fmt(&self, f: &mut JsonFormatter<'_, 'a>) {
        let span = self.expression.span();

        // `oxfmt-ignore` / `prettier-ignore` on the value: print leading comments
        // (including the marker) and then emit the value's source verbatim.
        if is_suppressed_before(f, span.start) {
            write!(f, FormatLeadingComments(span));
            write_suppressed_node(span, f);
            return;
        }

        write!(f, FormatLeadingComments(span));

        match self.expression {
            Expression::NullLiteral(_) => write!(f, "null"),
            Expression::BooleanLiteral(lit) => {
                write!(f, if lit.value { "true" } else { "false" });
            }
            Expression::NumericLiteral(lit) => literal::FmtJsonNumber { lit }.fmt(f),
            Expression::StringLiteral(lit) => literal::FmtJsonString { lit }.fmt(f),
            Expression::ArrayExpression(arr) => {
                array::FmtJsonArray { array: arr }.fmt(f);
            }
            Expression::ObjectExpression(obj) => {
                object::FmtJsonObject { object: obj }.fmt(f);
            }
            // `-9876.54321`, `+123`, `-Infinity`, etc. Prettier's `json` parser routes
            // through the JS estree printer, which keeps both `+` and `-` operators
            // while recursing into the argument so the inner number is normalized
            // (`-1.0e+2` → `-1.0e2`).
            Expression::UnaryExpression(unary) => {
                write!(f, text(unary.operator.as_str()));
                FmtJsonValue { expression: &unary.argument }.fmt(f);
            }
            // Anything else is not valid JSON; emit the source slice verbatim as a fallback.
            _ => write_source_slice(self.expression.span(), f),
        }
    }
}

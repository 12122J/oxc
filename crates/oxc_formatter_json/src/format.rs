use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_formatter_core::{
    Buffer, Document, Format, FormatState, Formatted, VecBuffer, builders::hard_line_break, write,
};
use oxc_span::GetSpan;

use crate::{
    comments::write_trailing_inside_comments,
    context::JsonFormatContext,
    options::JsonFormatOptions,
    parse::parse_json,
    print::{FmtJsonValue, JsonFormatter},
};

/// Parse `source` as JSON and build its formatter IR.
///
/// # Errors
/// Returns an [`OxcDiagnostic`] when the parser rejects `source`.
pub fn format<'a>(
    allocator: &'a Allocator,
    source: &str,
    options: JsonFormatOptions,
) -> Result<Formatted<'a, JsonFormatContext<'a>>, OxcDiagnostic> {
    let parsed = parse_json(allocator, source, options.variant)?;

    let context = JsonFormatContext::new(options, parsed.wrapped_source, parsed.comments);
    let mut state = FormatState::new(context, allocator);
    let mut buffer = VecBuffer::new(&mut state);

    write!(&mut buffer, FormatJsonRoot { expression: parsed.expression });

    let elements = buffer.into_vec();
    let document = Document::new(elements, Vec::new());
    document.propagate_expand();

    Ok(Formatted::new(document, state.into_context()))
}

// ---

/// Emits the root expression (when present) followed by any trailing comments
/// at the end of the document.
struct FormatJsonRoot<'a, 'b> {
    expression: Option<&'b Expression<'a>>,
}

impl<'a> Format<'a, JsonFormatContext<'a>> for FormatJsonRoot<'a, '_> {
    fn fmt(&self, f: &mut JsonFormatter<'_, 'a>) {
        let trailing_anchor = if let Some(expression) = self.expression {
            FmtJsonValue { expression }.fmt(f);
            expression.span().end
        } else {
            // Comments-only source: emit pending comments from the start of the source.
            0
        };
        let trailing = f.context().take_remaining_comments();
        write_trailing_inside_comments(trailing, trailing_anchor, f);

        // POSIX convention: every formatted file ends with a newline.
        // Prettier does the same for all parsers.
        write!(f, hard_line_break());
    }
}

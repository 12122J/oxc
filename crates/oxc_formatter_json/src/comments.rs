//! Minimal comment hoisting for JSON.
//!
//! JSON has only line and block comments and no JSDoc, type cast, or suppression annotations,
//! so a simple span-range based assignment is sufficient. The cursor in
//! [`crate::context::JsonFormatContext`] is advanced as comments are emitted, ensuring each
//! is printed exactly once.

use oxc_ast::Comment;
use oxc_formatter_core::{
    Buffer, Format,
    builders::{empty_line, hard_line_break, space, text},
    write,
};
use oxc_span::Span;

use crate::{context::JsonFormatContext, print::JsonFormatter};

/// Emit a single comment using the raw bytes from the wrapped source.
pub fn write_single_comment(comment: &Comment, f: &mut JsonFormatter<'_, '_>) {
    let span = comment.span;
    let s = &f.context().source()[span.start as usize..span.end as usize];
    write!(f, text(s));
}

/// Emit comments that precede an AST value, preserving the source's vertical spacing
/// (0/1/blank) between each comment and the next position — another comment for
/// in-group separators, or `value_start` for the last comment's break to the value.
pub fn write_leading_comments(
    comments: &[Comment],
    value_start: u32,
    f: &mut JsonFormatter<'_, '_>,
) {
    let source = f.context().source();
    for (i, comment) in comments.iter().enumerate() {
        write_single_comment(comment, f);
        let next_pos = comments.get(i + 1).map_or(value_start, |c| c.span.start);
        write_gap(&source.as_bytes()[comment.span.end as usize..next_pos as usize], f);
    }
}

/// Counts `\n` bytes in `slice`. Centralizes the `naive_bytecount` lint suppression
/// for the formatter's per-gap newline scans, which always operate on tiny whitespace
/// slices where pulling in `bytecount` would be overkill.
#[expect(clippy::naive_bytecount, reason = "tiny slice, not worth a bytecount dep")]
pub fn count_newlines(slice: &[u8]) -> usize {
    slice.iter().filter(|&&b| b == b'\n').count()
}

/// Emits the formatter element that reproduces the vertical spacing implied by `gap`:
/// `space` for 0 newlines, `hard_line_break` for 1, `empty_line` for 2+ (blank line).
fn write_gap(gap: &[u8], f: &mut JsonFormatter<'_, '_>) {
    match count_newlines(gap) {
        0 => write!(f, space()),
        1 => write!(f, hard_line_break()),
        _ => write!(f, empty_line()),
    }
}

/// Emit dangling comments inside an empty container (the caller wraps the result in
/// [`oxc_formatter_core::builders::block_indent`] or similar).
pub fn write_dangling_comments(comments: &[Comment], f: &mut JsonFormatter<'_, '_>) {
    for (i, comment) in comments.iter().enumerate() {
        if i > 0 {
            write!(f, hard_line_break());
        }
        write_single_comment(comment, f);
    }
}

/// Emit comments that sit between the last child of a container and its closing delimiter.
///
/// Like [`write_leading_comments`], preserves the source's vertical spacing (0/1/blank)
/// in front of each comment. `lower_bound` is the position immediately after the last
/// emitted content (typically the container's last child's `span.end`) and seeds the
/// gap measurement for the first comment.
pub fn write_trailing_inside_comments(
    comments: &[Comment],
    lower_bound: u32,
    f: &mut JsonFormatter<'_, '_>,
) {
    let source = f.context().source();
    let mut prev_end = lower_bound;
    for comment in comments {
        write_gap(&source.as_bytes()[prev_end as usize..comment.span.start as usize], f);
        write_single_comment(comment, f);
        prev_end = comment.span.end;
    }
}

/// Returns `true` if `comment` is an ignore marker (`oxfmt-ignore` / `prettier-ignore`).
/// Mirrors `oxc_formatter`'s suppression rule so JSON honors the same authoring convention
/// as JS/TS.
pub fn is_suppression_comment(source: &str, comment: &Comment) -> bool {
    let cs = comment.content_span();
    let body = &source[cs.start as usize..cs.end as usize];
    oxc_formatter_core::util::is_suppression_marker(body)
}

/// Returns `true` if any pending comment up to `before` is a suppression marker.
/// `before` is typically the next AST node's `span.start`.
pub fn is_suppressed_before(f: &JsonFormatter<'_, '_>, before: u32) -> bool {
    let source = f.context().source();
    f.context().comments_before_iter(before).any(|c| is_suppression_comment(source, c))
}

/// Emits a node's source slice verbatim and marks any comments inside it as printed,
/// so they aren't re-emitted later. Use for `oxfmt-ignore` / `prettier-ignore` blocks.
pub fn write_suppressed_node(span: Span, f: &mut JsonFormatter<'_, '_>) {
    let s = &f.context().source()[span.start as usize..span.end as usize];
    write!(f, text(s));
    // Drain (and discard) any comments inside the suppressed span — they've already
    // been emitted as part of the verbatim text.
    let _ = f.context().take_comments_before(span.end);
}

/// `Format` adapter that drains and prints all pending comments ending at or before
/// `span.start`. Lets callers replace the 3-line `take_comments_before` + `if !empty`
/// dance with `write!(f, [FormatLeadingComments(span), value])`.
pub struct FormatLeadingComments(pub Span);

impl<'a> Format<'a, JsonFormatContext<'a>> for FormatLeadingComments {
    fn fmt(&self, f: &mut JsonFormatter<'_, 'a>) {
        let leading = f.context().take_comments_before(self.0.start);
        write_leading_comments(leading, self.0.start, f);
    }
}

/// `Format` adapter that drains comments before `upper_bound` (typically the container's
/// closing-delimiter position) and writes them. `lower_bound` is the position right after
/// the last emitted child so the first comment's gap can be measured for blank-line
/// preservation; pass `upper_bound` when there is no prior child.
pub struct FormatTrailingInsideComments {
    pub lower_bound: u32,
    pub upper_bound: u32,
}

impl<'a> Format<'a, JsonFormatContext<'a>> for FormatTrailingInsideComments {
    fn fmt(&self, f: &mut JsonFormatter<'_, 'a>) {
        let trailing = f.context().take_comments_before(self.upper_bound);
        write_trailing_inside_comments(trailing, self.lower_bound, f);
    }
}

use std::cell::Cell;

use oxc_ast::Comment;
use oxc_formatter_core::FormatContext;

use crate::options::JsonFormatOptions;

/// Formatting context for JSON.
///
/// `comment_cursor` is interior-mutable because the `Format` trait dispatches through
/// `&self` references; it advances as comments are emitted so each is printed exactly once.
pub struct JsonFormatContext<'a> {
    options: JsonFormatOptions,
    source_code: &'a str,
    comments: &'a [Comment],
    comment_cursor: Cell<usize>,
}

impl<'a> JsonFormatContext<'a> {
    pub fn new(options: JsonFormatOptions, source_code: &'a str, comments: &'a [Comment]) -> Self {
        Self { options, source_code, comments, comment_cursor: Cell::new(0) }
    }

    /// Returns the source text with the arena lifetime (vs the trait's borrow-elided `&str`).
    /// Slices taken via this method don't have to be re-allocated for `text(...)`.
    pub fn source(&self) -> &'a str {
        self.source_code
    }

    /// Returns comments yet to be printed whose `span.end <= upper_bound`.
    /// Advances the cursor past them so they won't be returned again.
    pub fn take_comments_before(&self, upper_bound: u32) -> &'a [Comment] {
        let start = self.comment_cursor.get();
        let mut end = start;
        while end < self.comments.len() && self.comments[end].span.end <= upper_bound {
            end += 1;
        }
        self.comment_cursor.set(end);
        &self.comments[start..end]
    }

    /// Drains all remaining unprinted comments and returns them.
    pub fn take_remaining_comments(&self) -> &'a [Comment] {
        let start = self.comment_cursor.get();
        self.comment_cursor.set(self.comments.len());
        &self.comments[start..]
    }

    /// Iterator over unprinted comments whose `span.end <= upper_bound`.
    /// Does **not** advance the cursor — callers that want to mark these as
    /// printed must call [`Self::take_comments_before`] instead.
    ///
    /// Mirrors `oxc_formatter::formatter::comments::Comments::comments_before_iter`
    /// so suppression / leading-comment checks can compose `.any(...)` / `.next()`
    /// directly and short-circuit.
    pub fn comments_before_iter(&self, upper_bound: u32) -> impl Iterator<Item = &'a Comment> {
        let start = self.comment_cursor.get();
        self.comments[start..].iter().take_while(move |c| c.span.end <= upper_bound)
    }

    /// Returns `true` if at least one unprinted comment satisfies
    /// `span.end <= upper_bound`. Short-circuits on the first match.
    pub fn has_comments_before(&self, upper_bound: u32) -> bool {
        self.comments_before_iter(upper_bound).next().is_some()
    }
}

impl FormatContext for JsonFormatContext<'_> {
    type Options = JsonFormatOptions;

    fn options(&self) -> &Self::Options {
        &self.options
    }

    fn source_code(&self) -> &str {
        self.source_code
    }
}

use oxc_allocator::{Allocator, Vec as ArenaVec};
use oxc_ast::{
    Comment,
    ast::{Expression, Statement},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

use crate::options::JsonVariant;

/// Result of [`parse_json`].
/// All borrows are arena-lifetime,
/// so the parser's `Program` does not need to be kept alive by the caller.
pub struct ParsedJson<'a> {
    /// `None` when `source` contains only comments and whitespace.
    pub expression: Option<&'a Expression<'a>>,
    /// Sorted comments. Spans are in [`Self::wrapped_source`] coordinates.
    pub comments: &'a [Comment],
    /// Either:
    /// - `"(" + source + "\n)"` (normal path)
    /// - or the original `source` (comments-only fallback).
    ///
    /// All AST and comment spans index into this.
    pub wrapped_source: &'a str,
}

/// Parse a JSON document into a single arena-resident expression.
///
/// JSON object literals like `{"a":1}` are syntax errors when parsed as a script
/// (the leading `{` starts a `BlockStatement`),
/// so we wrap the source in `(...)` to force expression context.
/// `preserve_parens: false` keeps the wrapping `ParenthesizedExpression` out of the AST.
/// The trailing `\n` before `)` prevents a closing paren
/// from being swallowed by a trailing line comment in `source`.
///
/// If the wrapped parse fails,
/// we retry without the wrap and accept the result only when it contains no statements.
/// i.e. `source` is comments / whitespace only.
/// This lets comment-only JSON files round-trip without changing the normal path's cost.
///
/// # Errors
/// Returns an [`OxcDiagnostic`] if `source` has syntax errors that aren't explained
/// by the comments-only fallback, or when `variant` is [`JsonVariant::JsonStringify`]
/// and comments are present.
pub fn parse_json<'a>(
    allocator: &'a Allocator,
    source: &str,
    variant: JsonVariant,
) -> Result<ParsedJson<'a>, OxcDiagnostic> {
    let wrapped_source: &'a str = allocator.alloc_concat_strs_array(["(", source, "\n)"]);

    let ret = Parser::new(allocator, wrapped_source, SourceType::default())
        .with_options(ParseOptions { preserve_parens: false, ..ParseOptions::default() })
        .parse();

    if !ret.errors.is_empty() || ret.panicked {
        // The wrap turns a comments-only `source` into `(\n// ...\n)`, which is
        // an empty-parens syntax error. Re-parse without the wrap: if there are
        // no statements, `source` really was comments / whitespace only.
        if let Some(parsed) = try_parse_comments_only(allocator, source) {
            reject_comments_in_json_stringify(variant, parsed.comments)?;
            return Ok(parsed);
        }
        if let Some(err) = ret.errors.into_iter().next() {
            return Err(err);
        }
        return Err(OxcDiagnostic::error("Failed to parse JSON source"));
    }

    let mut program = ret.program;

    reject_comments_in_json_stringify(variant, &program.comments)?;

    // `Vec::into_arena_slice` consumes the stack-resident `Vec` header and
    // exposes its arena-resident storage as `&'a [_]`.
    // This is needed so neither the returned expression reference
    // nor the comments slice borrow from the local `program`.
    let comments =
        std::mem::replace(&mut program.comments, ArenaVec::new_in(allocator)).into_arena_slice();
    let body: &'a [Statement<'a>] =
        std::mem::replace(&mut program.body, ArenaVec::new_in(allocator)).into_arena_slice();

    // The wrap source guarantees exactly one top-level `ExpressionStatement`
    let stmt = body.first().ok_or_else(|| OxcDiagnostic::error("Empty JSON source"))?;
    let Statement::ExpressionStatement(expr_stmt) = stmt else {
        return Err(OxcDiagnostic::error("Expected a single expression at the top level"));
    };

    Ok(ParsedJson { expression: Some(&expr_stmt.expression), comments, wrapped_source })
}

/// Fallback path for the wrapped-parse failure: try parsing `source` as-is and accept
/// it only when there are no statements (i.e. comments / whitespace only).
fn try_parse_comments_only<'a>(allocator: &'a Allocator, source: &str) -> Option<ParsedJson<'a>> {
    // `Parser::new` ties `source_text` to the arena lifetime; copy `source` into the
    // arena so the resulting comment spans index into a string that outlives `ret`.
    let bare_source: &'a str = allocator.alloc_str(source);

    let ret = Parser::new(allocator, bare_source, SourceType::default()).parse();
    if !ret.errors.is_empty() || ret.panicked || !ret.program.body.is_empty() {
        return None;
    }

    let mut program = ret.program;
    let comments =
        std::mem::replace(&mut program.comments, ArenaVec::new_in(allocator)).into_arena_slice();

    Some(ParsedJson { expression: None, comments, wrapped_source: bare_source })
}

fn reject_comments_in_json_stringify(
    variant: JsonVariant,
    comments: &[Comment],
) -> Result<(), OxcDiagnostic> {
    if matches!(variant, JsonVariant::JsonStringify) && !comments.is_empty() {
        return Err(OxcDiagnostic::error("Comments are not allowed in `json-stringify`"));
    }
    Ok(())
}

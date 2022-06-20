//! `completions` crate provides utilities for generating completions of user input.

mod completions;
mod config;
mod context;
mod item;
mod render;

#[cfg(test)]
mod tests;
mod snippet;

use ide_db::{
    base_db::FilePosition,
    helpers::mod_path_to_ast,
    imports::{
        import_assets::NameToImport,
        insert_use::{self, ImportScope},
    },
    items_locator, RootDatabase,
};
use syntax::algo;
use text_edit::TextEdit;

use crate::{
    completions::Completions,
    context::{CompletionContext, IdentContext, NameKind, NameRefContext, NameRefKind},
};

pub use crate::{
    config::{CallableSnippets, CompletionConfig},
    item::{
        CompletionItem, CompletionItemKind, CompletionRelevance, CompletionRelevancePostfixMatch,
    },
    snippet::{Snippet, SnippetScope},
};

//FIXME: split the following feature into fine-grained features.

// Feature: Magic Completions
//
// In addition to usual reference completion, rust-analyzer provides some ✨magic✨
// completions as well:
//
// Keywords like `if`, `else` `while`, `loop` are completed with braces, and cursor
// is placed at the appropriate position. Even though `if` is easy to type, you
// still want to complete it, to get ` { }` for free! `return` is inserted with a
// space or `;` depending on the return type of the function.
//
// When completing a function call, `()` are automatically inserted. If a function
// takes arguments, the cursor is positioned inside the parenthesis.
//
// There are postfix completions, which can be triggered by typing something like
// `foo().if`. The word after `.` determines postfix completion. Possible variants are:
//
// - `expr.if` -> `if expr {}` or `if let ... {}` for `Option` or `Result`
// - `expr.match` -> `match expr {}`
// - `expr.while` -> `while expr {}` or `while let ... {}` for `Option` or `Result`
// - `expr.ref` -> `&expr`
// - `expr.refm` -> `&mut expr`
// - `expr.let` -> `let $0 = expr;`
// - `expr.letm` -> `let mut $0 = expr;`
// - `expr.not` -> `!expr`
// - `expr.dbg` -> `dbg!(expr)`
// - `expr.dbgr` -> `dbg!(&expr)`
// - `expr.call` -> `(expr)`
//
// There also snippet completions:
//
// .Expressions
// - `pd` -> `eprintln!(" = {:?}", );`
// - `ppd` -> `eprintln!(" = {:#?}", );`
//
// .Items
// - `tfn` -> `#[test] fn feature(){}`
// - `tmod` ->
// ```rust
// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_name() {}
// }
// ```
//
// And the auto import completions, enabled with the `rust-analyzer.completion.autoimport.enable` setting and the corresponding LSP client capabilities.
// Those are the additional completion options with automatic `use` import and options from all project importable items,
// fuzzy matched against the completion input.
//
// image::https://user-images.githubusercontent.com/48062697/113020667-b72ab880-917a-11eb-8778-716cf26a0eb3.gif[]

/// Main entry point for completion. We run completion as a two-phase process.
///
/// First, we look at the position and collect a so-called `CompletionContext.
/// This is a somewhat messy process, because, during completion, syntax tree is
/// incomplete and can look really weird.
///
/// Once the context is collected, we run a series of completion routines which
/// look at the context and produce completion items. One subtlety about this
/// phase is that completion engine should not filter by the substring which is
/// already present, it should give all possible variants for the identifier at
/// the caret. In other words, for
///
/// ```no_run
/// fn f() {
///     let foo = 92;
///     let _ = bar$0
/// }
/// ```
///
/// `foo` *should* be present among the completion variants. Filtering by
/// identifier prefix/fuzzy match should be done higher in the stack, together
/// with ordering of completions (currently this is done by the client).
///
/// # Speculative Completion Problem
///
/// There's a curious unsolved problem in the current implementation. Often, you
/// want to compute completions on a *slightly different* text document.
///
/// In the simplest case, when the code looks like `let x = `, you want to
/// insert a fake identifier to get a better syntax tree: `let x = complete_me`.
///
/// We do this in `CompletionContext`, and it works OK-enough for *syntax*
/// analysis. However, we might want to, eg, ask for the type of `complete_me`
/// variable, and that's where our current infrastructure breaks down. salsa
/// doesn't allow such "phantom" inputs.
///
/// Another case where this would be instrumental is macro expansion. We want to
/// insert a fake ident and re-expand code. There's `expand_speculative` as a
/// work-around for this.
///
/// A different use-case is completion of injection (examples and links in doc
/// comments). When computing completion for a path in a doc-comment, you want
/// to inject a fake path expression into the item being documented and complete
/// that.
///
/// IntelliJ has CodeFragment/Context infrastructure for that. You can create a
/// temporary PSI node, and say that the context ("parent") of this node is some
/// existing node. Asking for, eg, type of this `CodeFragment` node works
/// correctly, as the underlying infrastructure makes use of contexts to do
/// analysis.
pub fn completions(
    db: &RootDatabase,
    config: &CompletionConfig,
    position: FilePosition,
    trigger_character: Option<char>,
) -> Option<Completions> {
    let ctx = &CompletionContext::new(db, position, config)?;
    let mut completions = Completions::default();

    // prevent `(` from triggering unwanted completion noise
    if trigger_character == Some('(') {
        if let IdentContext::NameRef(NameRefContext { kind: NameRefKind::Path(path_ctx), .. }) =
            &ctx.ident_ctx
        {
            completions::vis::complete_vis_path(&mut completions, ctx, path_ctx);
        }
        // prevent `(` from triggering unwanted completion noise
        return Some(completions);
    }

    {
        let acc = &mut completions;

        match &ctx.ident_ctx {
            IdentContext::Name(name_ctx) => {
                completions::field::complete_field_list_record_variant(acc, ctx, name_ctx);
                completions::item_list::trait_impl::complete_trait_impl_name(acc, ctx, name_ctx);
                completions::mod_::complete_mod(acc, ctx, name_ctx);
                if let NameKind::IdentPat(pattern_ctx) = &name_ctx.kind {
                    completions::flyimport::import_on_the_fly_pat(acc, ctx, pattern_ctx);
                    completions::fn_param::complete_fn_param(acc, ctx, pattern_ctx);
                    completions::pattern::complete_pattern(acc, ctx, pattern_ctx);
                    completions::record::complete_record_pattern_fields(acc, ctx, pattern_ctx);
                }
            }
            IdentContext::NameRef(name_ctx @ NameRefContext { kind, .. }) => {
                completions::item_list::trait_impl::complete_trait_impl_name_ref(
                    acc, ctx, name_ctx,
                );
                completions::use_::complete_use_tree(acc, ctx, name_ctx);

                match kind {
                    NameRefKind::Path(path_ctx) => {
                        completions::attribute::complete_attribute(acc, ctx, path_ctx);
                        completions::attribute::complete_derive(acc, ctx, path_ctx);
                        completions::dot::complete_undotted_self(acc, ctx, path_ctx);
                        completions::expr::complete_expr_path(acc, ctx, path_ctx);
                        completions::field::complete_field_list_tuple_variant(acc, ctx, path_ctx);
                        completions::flyimport::import_on_the_fly_path(acc, ctx, path_ctx);
                        completions::item_list::complete_item_list(acc, ctx, path_ctx);
                        completions::pattern::pattern_path_completion(acc, ctx, path_ctx);
                        completions::r#type::complete_inferred_type(acc, ctx, path_ctx);
                        completions::r#type::complete_type_path(acc, ctx, path_ctx);
                        completions::record::complete_record_expr_func_update(acc, ctx, path_ctx);
                        completions::snippet::complete_expr_snippet(acc, ctx, path_ctx);
                        completions::snippet::complete_item_snippet(acc, ctx, path_ctx);
                        completions::vis::complete_vis_path(acc, ctx, path_ctx);
                    }
                    NameRefKind::DotAccess(dot_access) => {
                        completions::flyimport::import_on_the_fly_dot(acc, ctx, dot_access);
                        completions::dot::complete_dot(acc, ctx, dot_access);
                        completions::postfix::complete_postfix(acc, ctx, dot_access);
                    }
                    NameRefKind::Keyword(item) => {
                        completions::keyword::complete_special_keywords(acc, ctx, item);
                    }
                    NameRefKind::RecordExpr(record_expr) => {
                        completions::record::complete_record_expr_fields_record_expr(
                            acc,
                            ctx,
                            record_expr,
                        );
                    }
                    NameRefKind::Pattern(pattern_ctx) => {
                        completions::flyimport::import_on_the_fly_pat(acc, ctx, pattern_ctx);
                        completions::fn_param::complete_fn_param(acc, ctx, pattern_ctx);
                        completions::pattern::complete_pattern(acc, ctx, pattern_ctx);
                        completions::record::complete_record_pattern_fields(acc, ctx, pattern_ctx);
                    }
                }
            }
            IdentContext::Lifetime(lifetime_ctx) => {
                completions::lifetime::complete_label(acc, ctx, lifetime_ctx);
                completions::lifetime::complete_lifetime(acc, ctx, lifetime_ctx);
            }
            IdentContext::String { original, expanded: Some(expanded) } => {
                completions::extern_abi::complete_extern_abi(acc, ctx, expanded);
                completions::format_string::format_string(acc, ctx, original, expanded);
            }
            IdentContext::UnexpandedAttrTT { fake_attribute_under_caret: Some(attr) } => {
                completions::attribute::complete_known_attribute_input(acc, ctx, attr);
            }
            IdentContext::UnexpandedAttrTT { .. } | IdentContext::String { .. } => (),
        }
    }

    Some(completions)
}

/// Resolves additional completion data at the position given.
/// This is used for import insertion done via completions like flyimport and custom user snippets.
pub fn resolve_completion_edits(
    db: &RootDatabase,
    config: &CompletionConfig,
    position: FilePosition,
    imports: impl IntoIterator<Item = (String, String)>,
) -> Option<Vec<TextEdit>> {
    let _p = profile::span("resolve_completion_edits");
    let ctx = CompletionContext::new(db, position, config)?;
    let position_for_import = &ctx.original_token.parent()?;
    let scope = ImportScope::find_insert_use_container(position_for_import, &ctx.sema)?;

    let current_module = ctx.sema.scope(position_for_import)?.module();
    let current_crate = current_module.krate();
    let new_ast = scope.clone_for_update();
    let mut import_insert = TextEdit::builder();

    imports.into_iter().for_each(|(full_import_path, imported_name)| {
        let items_with_name = items_locator::items_with_name(
            &ctx.sema,
            current_crate,
            NameToImport::exact_case_sensitive(imported_name),
            items_locator::AssocItemSearch::Include,
            Some(items_locator::DEFAULT_QUERY_SEARCH_LIMIT.inner()),
        );
        let import = items_with_name
            .filter_map(|candidate| {
                current_module.find_use_path_prefixed(db, candidate, config.insert_use.prefix_kind)
            })
            .find(|mod_path| mod_path.to_string() == full_import_path);
        if let Some(import_path) = import {
            insert_use::insert_use(&new_ast, mod_path_to_ast(&import_path), &config.insert_use);
        }
    });

    algo::diff(scope.as_syntax_node(), new_ast.as_syntax_node()).into_text_edit(&mut import_insert);
    Some(vec![import_insert.finish()])
}

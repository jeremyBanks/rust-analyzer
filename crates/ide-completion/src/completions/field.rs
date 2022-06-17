//! Completion of field list position.

use crate::{
    context::{
        IdentContext, NameContext, NameKind, NameRefContext, NameRefKind, PathCompletionCtx,
        PathKind, Qualified, TypeLocation,
    },
    CompletionContext, Completions,
};

pub(crate) fn complete_field_list(acc: &mut Completions, ctx: &CompletionContext) {
    match &ctx.ident_ctx {
        IdentContext::Name(NameContext { kind: NameKind::RecordField, .. })
        | IdentContext::NameRef(NameRefContext {
            kind:
                Some(NameRefKind::Path(PathCompletionCtx {
                    has_macro_bang: false,
                    qualified: Qualified::No,
                    parent: None,
                    kind: PathKind::Type { location: TypeLocation::TupleField },
                    has_type_args: false,
                    ..
                })),
            ..
        }) => {
            if ctx.qualifier_ctx.vis_node.is_none() {
                let mut add_keyword = |kw, snippet| acc.add_keyword_snippet(ctx, kw, snippet);
                add_keyword("pub(crate)", "pub(crate)");
                add_keyword("pub(super)", "pub(super)");
                add_keyword("pub", "pub");
            }
        }
        _ => return,
    }
}

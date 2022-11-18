use crate::{AssistContext, AssistId, AssistKind, Assists};

pub(crate) fn fmt_imports(acc: &mut Assists, ctx: &AssistContext) -> Option<()> {
    let range = ctx.selection_trimmed();
    let covering = ctx.covering_element();
    acc.add(
        AssistId("fmt_imports", AssistKind::RefactorRewrite),
        format!("hello world"),
        range,
        |builder| {
            builder.insert(
                range.end(),
                format!("{covering:?}")
            );
        },
    );

    Some(())
}

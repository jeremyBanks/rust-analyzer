use crate::{AssistContext, AssistId, AssistKind, Assists};

pub(crate) fn fmt_imports(acc: &mut Assists, ctx: &AssistContext) -> Option<()> {
    let range = ctx.selection_trimmed();
    acc.add(
        AssistId("fmt_imports", AssistKind::RefactorRewrite),
        "Hello World Format Imports",
        range,
        |_builder| {
            eprintln!("hello world");
        },
    );

    Some(())
}

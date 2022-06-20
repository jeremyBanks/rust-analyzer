//! Completion of names from the current scope in type position.

use hir::{HirDisplay, ScopeDef};
use ide_db::FxHashSet;
use syntax::{ast, AstNode};

use crate::{
    context::{PathCompletionCtx, PathKind, Qualified, TypeAscriptionTarget, TypeLocation},
    render::render_type_inference,
    CompletionContext, Completions,
};

pub(crate) fn complete_type_path(
    acc: &mut Completions,
    ctx: &CompletionContext,
    path_ctx: &PathCompletionCtx,
) {
    let _p = profile::span("complete_type_path");

    let (location, qualified) = match path_ctx {
        PathCompletionCtx { kind: PathKind::Type { location }, qualified, .. } => {
            (location, qualified)
        }
        _ => return,
    };

    let scope_def_applicable = |def| {
        use hir::{GenericParam::*, ModuleDef::*};
        match def {
            ScopeDef::GenericParam(LifetimeParam(_)) | ScopeDef::Label(_) => false,
            // no values in type places
            ScopeDef::ModuleDef(Function(_) | Variant(_) | Static(_)) | ScopeDef::Local(_) => false,
            // unless its a constant in a generic arg list position
            ScopeDef::ModuleDef(Const(_)) | ScopeDef::GenericParam(ConstParam(_)) => {
                matches!(location, TypeLocation::GenericArgList(_))
            }
            ScopeDef::ImplSelfType(_) => {
                !matches!(location, TypeLocation::ImplTarget | TypeLocation::ImplTrait)
            }
            // Don't suggest attribute macros and derives.
            ScopeDef::ModuleDef(Macro(mac)) => mac.is_fn_like(ctx.db),
            // Type things are fine
            ScopeDef::ModuleDef(BuiltinType(_) | Adt(_) | Module(_) | Trait(_) | TypeAlias(_))
            | ScopeDef::AdtSelfType(_)
            | ScopeDef::Unknown
            | ScopeDef::GenericParam(TypeParam(_)) => true,
        }
    };

    let add_assoc_item = |acc: &mut Completions, item| match item {
        hir::AssocItem::Const(ct) if matches!(location, TypeLocation::GenericArgList(_)) => {
            acc.add_const(ctx, ct)
        }
        hir::AssocItem::Function(_) | hir::AssocItem::Const(_) => (),
        hir::AssocItem::TypeAlias(ty) => acc.add_type_alias(ctx, ty),
    };

    match qualified {
        Qualified::Infer => ctx
            .traits_in_scope()
            .0
            .into_iter()
            .flat_map(|it| hir::Trait::from(it).items(ctx.sema.db))
            .for_each(|item| add_assoc_item(acc, item)),
        Qualified::With { resolution: None, .. } => {}
        Qualified::With { resolution: Some(resolution), .. } => {
            // Add associated types on type parameters and `Self`.
            ctx.scope.assoc_type_shorthand_candidates(resolution, |_, alias| {
                acc.add_type_alias(ctx, alias);
                None::<()>
            });

            match resolution {
                hir::PathResolution::Def(hir::ModuleDef::Module(module)) => {
                    let module_scope = module.scope(ctx.db, Some(ctx.module));
                    for (name, def) in module_scope {
                        if scope_def_applicable(def) {
                            acc.add_resolution(ctx, name, def);
                        }
                    }
                }
                hir::PathResolution::Def(
                    def @ (hir::ModuleDef::Adt(_)
                    | hir::ModuleDef::TypeAlias(_)
                    | hir::ModuleDef::BuiltinType(_)),
                ) => {
                    let ty = match def {
                        hir::ModuleDef::Adt(adt) => adt.ty(ctx.db),
                        hir::ModuleDef::TypeAlias(a) => a.ty(ctx.db),
                        hir::ModuleDef::BuiltinType(builtin) => builtin.ty(ctx.db),
                        _ => return,
                    };

                    // XXX: For parity with Rust bug #22519, this does not complete Ty::AssocType.
                    // (where AssocType is defined on a trait, not an inherent impl)

                    ty.iterate_path_candidates(
                        ctx.db,
                        &ctx.scope,
                        &ctx.traits_in_scope().0,
                        Some(ctx.module),
                        None,
                        |item| {
                            add_assoc_item(acc, item);
                            None::<()>
                        },
                    );

                    // Iterate assoc types separately
                    ty.iterate_assoc_items(ctx.db, ctx.krate, |item| {
                        if let hir::AssocItem::TypeAlias(ty) = item {
                            acc.add_type_alias(ctx, ty)
                        }
                        None::<()>
                    });
                }
                hir::PathResolution::Def(hir::ModuleDef::Trait(t)) => {
                    // Handles `Trait::assoc` as well as `<Ty as Trait>::assoc`.
                    for item in t.items(ctx.db) {
                        add_assoc_item(acc, item);
                    }
                }
                hir::PathResolution::TypeParam(_) | hir::PathResolution::SelfType(_) => {
                    let ty = match resolution {
                        hir::PathResolution::TypeParam(param) => param.ty(ctx.db),
                        hir::PathResolution::SelfType(impl_def) => impl_def.self_ty(ctx.db),
                        _ => return,
                    };

                    let mut seen = FxHashSet::default();
                    ty.iterate_path_candidates(
                        ctx.db,
                        &ctx.scope,
                        &ctx.traits_in_scope().0,
                        Some(ctx.module),
                        None,
                        |item| {
                            // We might iterate candidates of a trait multiple times here, so deduplicate
                            // them.
                            if seen.insert(item) {
                                add_assoc_item(acc, item);
                            }
                            None::<()>
                        },
                    );
                }
                _ => (),
            }
        }
        Qualified::Absolute => acc.add_crate_roots(ctx),
        Qualified::No => {
            acc.add_nameref_keywords_with_colon(ctx);
            if let TypeLocation::TypeBound = location {
                ctx.process_all_names(&mut |name, res| {
                    let add_resolution = match res {
                        ScopeDef::ModuleDef(hir::ModuleDef::Macro(mac)) => mac.is_fn_like(ctx.db),
                        ScopeDef::ModuleDef(
                            hir::ModuleDef::Trait(_) | hir::ModuleDef::Module(_),
                        ) => true,
                        _ => false,
                    };
                    if add_resolution {
                        acc.add_resolution(ctx, name, res);
                    }
                });
                return;
            }
            if let TypeLocation::GenericArgList(Some(arg_list)) = location {
                if let Some(path_seg) = arg_list.syntax().parent().and_then(ast::PathSegment::cast)
                {
                    if path_seg.syntax().ancestors().find_map(ast::TypeBound::cast).is_some() {
                        if let Some(hir::PathResolution::Def(hir::ModuleDef::Trait(trait_))) =
                            ctx.sema.resolve_path(&path_seg.parent_path())
                        {
                            trait_.items_with_supertraits(ctx.sema.db).into_iter().for_each(|it| {
                                if let hir::AssocItem::TypeAlias(alias) = it {
                                    cov_mark::hit!(complete_assoc_type_in_generics_list);
                                    acc.add_type_alias_with_eq(ctx, alias)
                                }
                            });
                        }
                    }
                }
            }
            ctx.process_all_names(&mut |name, def| {
                if scope_def_applicable(def) {
                    acc.add_resolution(ctx, name, def);
                }
            });
        }
    }
}

pub(crate) fn complete_inferred_type(
    acc: &mut Completions,
    ctx: &CompletionContext,
    path_ctx: &PathCompletionCtx,
) -> Option<()> {
    let pat = match path_ctx {
        PathCompletionCtx {
            kind: PathKind::Type { location: TypeLocation::TypeAscription(ascription), .. },
            ..
        } if path_ctx.is_trivial_path() => ascription,
        _ => return None,
    };
    let x = match pat {
        TypeAscriptionTarget::Let(pat) | TypeAscriptionTarget::FnParam(pat) => {
            ctx.sema.type_of_pat(pat.as_ref()?)
        }
        TypeAscriptionTarget::Const(exp) | TypeAscriptionTarget::RetType(exp) => {
            ctx.sema.type_of_expr(exp.as_ref()?)
        }
    }?
    .adjusted();
    let ty_string = x.display_source_code(ctx.db, ctx.module.into()).ok()?;
    acc.add(render_type_inference(ty_string, ctx));
    None
}

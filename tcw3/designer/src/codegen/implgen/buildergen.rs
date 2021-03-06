use either::{Left, Right};
use quote::ToTokens;
use std::{fmt::Write, iter::repeat};

use super::super::{diag::Diag, sem};
use super::{
    analysis, initgen, iterutils::Iterutils, paths, Angle, CommaSeparated, CompBuilderTy, CompTy,
    Ctx, FactoryGenParamNameForField, FactorySetterForField, InnerValueField,
};
use crate::metadata;

pub fn gen_builder(
    analysis: &analysis::Analysis,
    dep_analysis: &initgen::DepAnalysis,
    ctx: &Ctx,
    item_meta2sem_map: &[usize],
    diag: &mut Diag,
    out: &mut String,
    scoped_out: &mut String,
) {
    let comp = ctx.cur_comp;
    let comp_ident = &comp.ident.sym;
    let comp_path = &comp.path;

    // The simple builder API does not have a builder type. Our codegen can't
    // generate it anyway.
    assert!(!comp.flags.contains(metadata::CompFlags::SIMPLE_BUILDER));

    let builder_vis = ctx.cur_meta_comp().builder_vis();

    let settable_fields = comp.items.iter().filter_map(|item| match item {
        sem::CompItemDef::Field(
            field
            @
            sem::FieldDef {
                accessors: sem::FieldAccessors { set: Some(_), .. },
                ..
            },
        ) => {
            assert!(field.field_ty != sem::FieldType::Wire);
            Some(field)
        }
        _ => None,
    });
    let optional_fields = settable_fields
        .clone()
        .filter(|field| field.value.is_some());
    let non_optional_fields = settable_fields
        .clone()
        .filter(|field| field.value.is_none());
    let num_non_optional_consts = non_optional_fields.clone().count();

    // `T_field1`, `T_field2`, ...
    let builder_ty_params = non_optional_fields
        .clone()
        .map(|field| FactoryGenParamNameForField(&field.ident.sym));

    // `u32`, `HView`, ...
    let builder_complete_ty_params = non_optional_fields
        .clone()
        .map(|field| field.ty.to_token_stream());

    use super::docgen::{gen_doc_attrs, CodegenInfoDoc, MdCode};
    writeln!(
        out,
        "{}",
        doc_attr!("The builder type of {} component.", MdCode(comp_ident))
    )
    .unwrap();
    writeln!(out, "{}", doc_attr!("")).unwrap();
    writeln!(out, "{}", CodegenInfoDoc(None, diag)).unwrap();

    writeln!(out, "#[allow(non_camel_case_types)]").unwrap();
    writeln!(
        out,
        "{vis} struct {ty}{gen} {{",
        vis = builder_vis.display(ctx.repo),
        ty = CompBuilderTy(&ctx.cur_comp.ident.sym),
        gen = if num_non_optional_consts != 0 {
            Left(Angle(CommaSeparated(builder_ty_params.clone())))
        } else {
            Right("")
        }
    )
    .unwrap();
    for field in settable_fields.clone() {
        writeln!(
            out,
            "    {ident}: {ty},",
            ident = InnerValueField(&field.ident.sym),
            ty = if field.value.is_some() {
                // optional
                Left(format!("{}<{}>", paths::OPTION, field.ty.to_token_stream()))
            } else {
                // non-optional - use a generics-based trick to enforce
                //                initialization
                Right(FactoryGenParamNameForField(&field.ident.sym))
            },
        )
        .unwrap();
    }
    writeln!(out, "}}").unwrap();

    // `ComponentBuilder::<Unset, ...>::new`
    // -------------------------------------------------------------------
    writeln!(scoped_out, "#[allow(non_camel_case_types)]").unwrap();
    writeln!(
        scoped_out,
        "impl {ident}{gen} {{",
        ident = CompBuilderTy(comp_path),
        gen = if num_non_optional_consts != 0 {
            Left(Angle(CommaSeparated(
                repeat(ctx.path_unset()).take(num_non_optional_consts),
            )))
        } else {
            Right("")
        }
    )
    .unwrap();

    writeln!(
        scoped_out,
        "    {}",
        doc_attr!("Construct {}.", MdCode(CompBuilderTy(comp_path)))
    )
    .unwrap();
    writeln!(
        scoped_out,
        "    {vis} fn new() -> Self {{",
        vis = builder_vis.display(ctx.repo)
    )
    .unwrap();
    writeln!(scoped_out, "        Self {{").unwrap();
    for field in settable_fields.clone() {
        writeln!(
            scoped_out,
            "            {ident}: {ty},",
            ident = InnerValueField(&field.ident.sym),
            ty = if field.value.is_some() {
                Left("None")
            } else {
                Right(ctx.path_unset())
            },
        )
        .unwrap();
    }
    writeln!(scoped_out, "        }}").unwrap();
    writeln!(scoped_out, "    }}").unwrap();
    writeln!(scoped_out, "}}").unwrap();

    // `ComponentBuilder::{with_*}`
    // -------------------------------------------------------------------
    writeln!(scoped_out, "#[allow(non_camel_case_types)]").unwrap();
    writeln!(
        scoped_out,
        "impl{gen} {ty}{gen} {{",
        ty = CompBuilderTy(comp_path),
        gen = if non_optional_fields.clone().next().is_some() {
            Left(Angle(CommaSeparated(builder_ty_params.clone())))
        } else {
            Right("")
        }
    )
    .unwrap();

    let gen_setter_doc = |scoped_out: &mut String, field: &sem::FieldDef<'_>| {
        writeln!(
            scoped_out,
            "    {}",
            doc_attr!("Set the value of {} property.", MdCode(&field.ident.sym))
        )
        .unwrap();
        writeln!(scoped_out, "    {}", doc_attr!("")).unwrap();
        gen_doc_attrs(&field.doc_attrs, "    ", scoped_out);
    };

    for field in optional_fields.clone() {
        // They just assign a new value to `Option<T>`
        gen_setter_doc(scoped_out, field);

        writeln!(
            scoped_out,
            "    {vis} fn {method}(self, {ident}: {ty}) -> Self {{",
            vis = field.accessors.set.as_ref().unwrap().vis,
            method = FactorySetterForField(&field.ident.sym),
            ident = field.ident.sym,
            ty = field.ty.to_token_stream(),
        )
        .unwrap();
        writeln!(
            scoped_out,
            "        Self {{ {field}: {some}({ident}), ..self }}",
            some = paths::SOME,
            field = InnerValueField(&field.ident.sym),
            ident = field.ident.sym,
        )
        .unwrap();
        writeln!(scoped_out, "    }}",).unwrap();
    }

    for (i, field) in non_optional_fields.clone().enumerate() {
        // They each change one type parameter of `ComponentBuilder`
        gen_setter_doc(scoped_out, field);

        let new_builder_ty = format!(
            "{ty}<{gen}>",
            ty = CompBuilderTy(comp_path),
            gen = CommaSeparated(
                builder_ty_params
                    .clone()
                    .map(Left)
                    .replace_at(i, Right(field.ty.to_token_stream()))
            )
        );
        writeln!(
            scoped_out,
            "    {vis} fn {method}(self, {ident}: {ty}) -> {new_bldr_ty} {{",
            vis = field.accessors.set.as_ref().unwrap().vis,
            method = FactorySetterForField(&field.ident.sym),
            ident = field.ident.sym,
            ty = field.ty.to_token_stream(),
            new_bldr_ty = new_builder_ty,
        )
        .unwrap();
        writeln!(
            scoped_out,
            "        {ty} {{ {fields} }}",
            ty = CompBuilderTy(comp_path),
            fields = CommaSeparated(settable_fields.clone().map(|field2| {
                if field2.ident.sym == field.ident.sym {
                    // Replace with the new value
                    format!(
                        "{}: {}",
                        InnerValueField(&field2.ident.sym),
                        field2.ident.sym
                    )
                } else {
                    // Use the old value
                    format!("{0}: self.{0}", InnerValueField(&field2.ident.sym),)
                }
            }))
        )
        .unwrap();
        writeln!(scoped_out, "    }}",).unwrap();
    }
    writeln!(scoped_out, "}}").unwrap();

    // `ComponentBuilder::<u32, ...>::build`
    // -------------------------------------------------------------------
    writeln!(
        scoped_out,
        "impl {ty}{gen} {{",
        ty = CompBuilderTy(comp_path),
        gen = if num_non_optional_consts != 0 {
            Left(Angle(CommaSeparated(builder_complete_ty_params)))
        } else {
            Right("")
        }
    )
    .unwrap();

    writeln!(
        scoped_out,
        "    {}",
        doc_attr!("Construct {}.", MdCode(CompTy(&comp.ident.sym)))
    )
    .unwrap();

    writeln!(
        scoped_out,
        "    {vis} fn build(self) -> {ty} {{",
        vis = builder_vis.display(ctx.repo),
        ty = comp_path,
    )
    .unwrap();
    initgen::gen_construct(analysis, dep_analysis, ctx, item_meta2sem_map, scoped_out);
    writeln!(scoped_out, "    }}").unwrap();
    writeln!(scoped_out, "}}").unwrap();
}

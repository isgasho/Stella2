use codemap_diagnostic::{Diagnostic, Level, SpanLabel, SpanStyle};
use log::debug;
use pathfinding::directed::{
    strongly_connected_components::strongly_connected_components,
    topological_sort::topological_sort,
};
use std::{fmt::Write, ops::Range};

use super::super::{diag::Diag, sem, EmittedError};
use super::{
    analysis, evalgen, fields, paths, CompBuilderTy, CompSharedTy, CompStateTy, CompTy, Ctx,
    EventInnerSubList, FactorySetterForField, InnerValueField, SetterMethod, TempVar,
};
use crate::metadata;

#[derive(Debug)]
enum DepNode {
    Field { item_i: usize },
    // Actually, this doesn't have to be a node because it could be just
    // initialized as a part of `Field`. Nevertheless, it's represented as
    // a node for better reporting of a circular reference.
    ObjInitField { item_i: usize, field_i: usize },
    This,
}

pub struct DepAnalysis {
    nodes: Vec<DepNode>,
    item2node_map: Vec<usize>,
    ordered_node_i_list: Vec<usize>,
}

impl DepAnalysis {
    pub fn new(
        analysis: &analysis::Analysis,
        ctx: &Ctx,
        item_meta2sem_map: &[usize],
        diag: &mut Diag,
    ) -> Result<Self, EmittedError> {
        analyze_dep(analysis, ctx, item_meta2sem_map, diag)
    }
}

/// Analyze dependencies between fields.
fn analyze_dep(
    analysis: &analysis::Analysis,
    ctx: &Ctx,
    item_meta2sem_map: &[usize],
    diag: &mut Diag,
) -> Result<DepAnalysis, EmittedError> {
    let comp = ctx.cur_comp;

    // Construct a dependency graph to find the initialization order
    // ----------------------------------------------------------------------
    let mut nodes = vec![DepNode::This];

    // Define nodes
    let mut item2node_map = Vec::with_capacity(comp.items.len());
    for (item_i, item) in comp.items.iter().enumerate() {
        item2node_map.push(nodes.len());

        match item {
            sem::CompItemDef::Field(item) => {
                nodes.push(DepNode::Field { item_i });

                if let Some(sem::DynExpr::ObjInit(init)) = &item.value {
                    // Add all fields
                    let num_fields = init.fields.len();
                    nodes.extend(
                        (0..num_fields).map(|field_i| DepNode::ObjInitField { item_i, field_i }),
                    );
                }
            }
            sem::CompItemDef::On(_) | sem::CompItemDef::Event(_) => {}
        }
    }

    // Find node dependencies
    let mut deps = Vec::new();

    let push_func_deps = |deps: &mut Vec<usize>, func: &sem::Func| {
        for func_input in func.inputs.iter() {
            match analysis.get_input(&func_input.input) {
                analysis::InputInfo::EventParam(_) => unreachable!(),
                analysis::InputInfo::Item(item_input) => {
                    let ind0 = item_input.indirections.first().unwrap();
                    let sem_item_i = item_meta2sem_map[ind0.item_i];
                    deps.push(item2node_map[sem_item_i]);
                }
                analysis::InputInfo::This => {
                    deps.push(0); // `DepNode::This`
                }
                analysis::InputInfo::Invalid => {}
            }
        }
    };

    let dep_ranges: Vec<Range<usize>> = nodes
        .iter()
        .enumerate()
        .map(|(node_i, node)| {
            let start = deps.len();

            match node {
                DepNode::This => {
                    // `this` depends on all fields
                    for (item_i, item) in comp.items.iter().enumerate() {
                        if let sem::CompItemDef::Field(_) = item {
                            deps.push(item2node_map[item_i]);
                        }
                    }
                }
                DepNode::Field { item_i } => {
                    match &comp.items[*item_i].field().unwrap().value {
                        None => {}
                        Some(sem::DynExpr::Func(func)) => {
                            push_func_deps(&mut deps, func);
                        }
                        Some(sem::DynExpr::ObjInit(_)) => {
                            // In `nodes`, this node is followed by zero or more
                            // `DepNode::ObjInitField` nodes
                            deps.extend((node_i + 1..nodes.len()).take_while(|&i| {
                                match &nodes[i] {
                                    DepNode::ObjInitField {
                                        item_i: item_i2, ..
                                    } if item_i2 == item_i => true,
                                    _ => false,
                                }
                            }));
                        }
                    }
                }
                DepNode::ObjInitField { item_i, field_i } => {
                    let field_item = comp.items[*item_i].field().unwrap();
                    let obj_init = field_item.value.as_ref().unwrap().obj_init().unwrap();
                    let field = &obj_init.fields[*field_i];
                    push_func_deps(&mut deps, &field.value);
                }
            }

            start..deps.len() // A range into `deps` representing `node`'s dependencies
        })
        .collect();

    let node_i_list: Vec<_> = (0..nodes.len()).collect();
    let node_depends_on = |&node_i: &usize| deps[dep_ranges[node_i].clone()].iter().copied();

    // Log the dependency
    if log::LevelFilter::Debug <= log::max_level() {
        debug!(
            "Planning field initialization for the component `{}`",
            comp.path
        );
        for (i, node) in nodes.iter().enumerate() {
            debug!(
                " [{}] {:?} → {:?}",
                i,
                node,
                node_depends_on(&i).collect::<Vec<_>>()
            );
        }
    }

    // Find a topological order
    let ordered_node_i_list = topological_sort(&node_i_list, node_depends_on);

    debug!("Initialization order = {:?}", ordered_node_i_list);

    let ordered_node_i_list = if let Ok(mut x) = ordered_node_i_list {
        x.reverse();
        x
    } else {
        // If none was found, find cycles and report them as an error.
        let sccs = strongly_connected_components(&node_i_list, node_depends_on);

        diag.emit(&[Diagnostic {
            level: Level::Error,
            message: format!(
                "A circular dependency was detected in the \
                 field initialization of `{}`",
                comp.path
            ),
            code: None,
            spans: comp
                .path
                .span
                .map(|span| SpanLabel {
                    span,
                    label: None,
                    style: SpanStyle::Primary,
                })
                .into_iter()
                .collect(),
        }]);

        let num_cycles = sccs.iter().filter(|scc| scc.len() > 1).count();

        for (i, scc) in sccs.iter().filter(|scc| scc.len() > 1).enumerate() {
            let codemap_spans: Vec<_> = scc
                .iter()
                .rev()
                .filter_map(|&x| match &nodes[x] {
                    DepNode::Field { item_i } => {
                        let field = comp.items[*item_i].field().unwrap();
                        Some((field.ident.span?, "initialization of this field"))
                    }
                    DepNode::ObjInitField { item_i, field_i } => {
                        let field = comp.items[*item_i].field().unwrap();
                        let obj_init = field.value.as_ref().unwrap().obj_init().unwrap();
                        let init_field = &obj_init.fields[*field_i];
                        Some((init_field.ident.span?, "initialization of this field"))
                    }
                    DepNode::This => Some((comp.path.span?, "`this` reference of the component")),
                })
                .enumerate()
                .map(|(i, (span, label))| SpanLabel {
                    span,
                    label: Some(format!("({}) {}", i + 1, label)),
                    style: SpanStyle::Primary,
                })
                .collect();

            diag.emit(&[Diagnostic {
                level: Level::Note,
                message: format!("Cycle (SCC) {} of {}", i + 1, num_cycles),
                code: None,
                spans: codemap_spans,
            }]);
        }

        let involves_this = sccs
            .iter()
            .filter(|scc| scc.len() > 1 && scc.contains(&0))
            .nth(0)
            .is_some();

        if involves_this {
            diag.emit(&[Diagnostic {
                level: Level::Note,
                message: "`this` is constructed after initializing all fields".to_string(),
                code: None,
                spans: vec![],
            }]);
        }

        return Err(EmittedError);
    };

    // The last node should be `this`
    assert_eq!(*ordered_node_i_list.last().unwrap(), 0);

    Ok(DepAnalysis {
        nodes,
        item2node_map,
        ordered_node_i_list,
    })
}

/// Generates construction code for a component. The generated expression
/// evaluates to the type named `CompTy(comp_ident)`.
///
/// Assumes settable fields are in `self` of type `xxxBuilder`.
pub fn gen_construct(
    analysis: &analysis::Analysis,
    dep_analysis: &DepAnalysis,
    ctx: &Ctx,
    item_meta2sem_map: &[usize],
    diag: &mut Diag,
    out: &mut String,
) {
    let comp = ctx.cur_comp;
    let comp_ident = &comp.ident.sym;

    let nodes = &dep_analysis.nodes[..];
    let item2node_map = &dep_analysis.item2node_map[..];
    let ordered_node_i_list = &dep_analysis.ordered_node_i_list[..];

    // Emit field initializers
    // ----------------------------------------------------------------------
    struct InitFuncInputGen<'a> {
        item2node_map: &'a [usize],
    }

    impl evalgen::FuncInputGen for InitFuncInputGen<'_> {
        fn gen_field_ref(&mut self, item_i: usize, by_ref: bool, out: &mut String) {
            let node_i = self.item2node_map[item_i];

            if by_ref {
                write!(out, "(&{})", TempVar(node_i)).unwrap();
            } else {
                write!(out, "{}::clone(&{})", paths::CLONE, TempVar(node_i)).unwrap();
            }
        }

        fn gen_this(&mut self, _out: &mut String) {
            // `this: ComponentType` is unavailable at this point
            unreachable!()
        }

        // `InitFuncInputGen` isn't used for event handlers, so the following
        // two methods are never called
        fn trigger_i(&mut self) -> usize {
            unreachable!()
        }
        fn gen_event_param(&mut self, _param_i: usize, _out: &mut String) {
            unreachable!()
        }
    }

    let mut func_input_gen = InitFuncInputGen {
        item2node_map: &item2node_map,
    };

    let var_state = TempVar("state");
    let var_shared = TempVar("shared");
    let var_this = TempVar(0); // `DepNode::This`
    for (i, node) in ordered_node_i_list.iter().map(|&i| (i, &nodes[i])) {
        let var = TempVar(i);
        match node {
            DepNode::This => {
                assert_eq!(var.0, var_this.0);

                // `struct ComponentTypeState`
                writeln!(out, "let {} = {} {{", var_state, CompStateTy(&comp_ident)).unwrap();
                for (i, item) in comp.items.iter().enumerate() {
                    let val = TempVar(item2node_map[i]);
                    match item {
                        sem::CompItemDef::Field(item) => match item.field_ty {
                            sem::FieldType::Const => {}
                            sem::FieldType::Wire | sem::FieldType::Prop => {
                                writeln!(
                                    out,
                                    "    {ident}: {val},",
                                    ident = InnerValueField(&item.ident.sym),
                                    val = val,
                                )
                                .unwrap();
                            }
                        },
                        _ => {}
                    }
                }
                writeln!(out, "}};").unwrap();

                // `struct ComponentTypeShared`
                writeln!(out, "let {} = {} {{", var_shared, CompSharedTy(&comp_ident)).unwrap();
                for (i, item) in comp.items.iter().enumerate() {
                    let val = TempVar(item2node_map[i]);
                    match item {
                        sem::CompItemDef::Field(item) => match item.field_ty {
                            sem::FieldType::Wire => {}
                            sem::FieldType::Prop => {
                                writeln!(
                                    out,
                                    "    {ident}: {def}::default(),",
                                    ident = InnerValueField(&item.ident.sym),
                                    def = paths::DEFAULT,
                                )
                                .unwrap();
                            }
                            sem::FieldType::Const => {
                                writeln!(
                                    out,
                                    "    {ident}: {val},",
                                    ident = InnerValueField(&item.ident.sym),
                                    val = val,
                                )
                                .unwrap();
                            }
                        },
                        sem::CompItemDef::Event(item) => {
                            writeln!(
                                out,
                                "    {ident}: {def}::default(),",
                                ident = EventInnerSubList(&item.ident.sym),
                                def = paths::DEFAULT,
                            )
                            .unwrap();
                        }
                        _ => {}
                    }
                }
                writeln!(
                    out,
                    "    {field}: {refcell}::new({val}),",
                    field = fields::STATE,
                    refcell = paths::REF_CELL,
                    val = var_state,
                )
                .unwrap();
                writeln!(out, "}};").unwrap();

                // `struct ComponentType`
                writeln!(out, "let {} = {} {{", var_this, CompTy(&comp_ident)).unwrap();
                writeln!(
                    out,
                    "    {field}: {rc}::new({shared})",
                    field = fields::SHARED,
                    rc = paths::RC,
                    shared = var_shared
                )
                .unwrap();
                writeln!(out, "}};").unwrap();
            } // DepNode::This

            DepNode::Field { item_i } => {
                let field = comp.items[*item_i].field().unwrap();
                write!(out, "let {} = ", var).unwrap();

                if field.value.is_none() {
                    // Mandatory field - the value is always available
                    // from `ComponentTypeBuilder`
                    writeln!(
                        out,
                        "self.{field};",
                        field = InnerValueField(&field.ident.sym)
                    )
                    .unwrap();
                    continue;
                }

                let is_settable = field.accessors.set.is_some();
                if is_settable {
                    // Check if the value is available from `ComponentTypeBuilder`
                    let var_tmp = TempVar("given_value");
                    writeln!(
                        out,
                        "if let {some}({t}) = self.{field} {{ {t} }} else {{",
                        some = paths::SOME,
                        t = var_tmp,
                        field = InnerValueField(&field.ident.sym)
                    )
                    .unwrap();
                }

                match field.value.as_ref().unwrap() {
                    sem::DynExpr::Func(func) => {
                        evalgen::gen_func_eval(
                            func,
                            analysis,
                            ctx,
                            item_meta2sem_map,
                            &mut func_input_gen,
                            out,
                        );
                    }
                    sem::DynExpr::ObjInit(init) => {
                        // Find the component we are constructing. The field's
                        // type is guaranteed to match the component's type
                        // because we do not allow explicitly specifying the type
                        // when `ObjInit` is in use.
                        let meta_item_i =
                            item_meta2sem_map.iter().position(|i| i == item_i).unwrap();
                        let meta_field = ctx.cur_meta_comp().items[meta_item_i].field().unwrap();

                        if let Some(ty) = &meta_field.ty {
                            let initer_map = check_obj_init(ctx.repo.comp_by_ref(ty), init, diag);

                            gen_obj_init(
                                ctx.repo.comp_by_ref(ty),
                                init,
                                analysis,
                                ctx,
                                item_meta2sem_map,
                                &mut func_input_gen,
                                &initer_map,
                                out,
                            );
                        } else {
                            diag.emit(&[Diagnostic {
                                level: Level::Error,
                                message: format!("`{}` does not refer to a component", init.path),
                                code: None,
                                spans: init
                                    .path
                                    .span
                                    .map(|span| SpanLabel {
                                        span,
                                        label: None,
                                        style: SpanStyle::Primary,
                                    })
                                    .into_iter()
                                    .collect(),
                            }]);

                            write!(out, "panic!(\"codegen failed\")").unwrap();
                        }
                    }
                }

                if is_settable {
                    writeln!(out, "\n}};").unwrap(); // close the `if` block
                } else {
                    writeln!(out, ";").unwrap();
                }
            } // DepNode::Field

            DepNode::ObjInitField { .. } => {
                // It's a part of `Field` and initialized in there
            } // DepNode::ObjInitField
        }
    }

    // TODO: Setup event handlers (maybe in another source file?)

    writeln!(out, "{}", var_this).unwrap();
}

/// Analyze `ObjInit` and report errors if any.
///
/// Returns a multi-map from indices into `comp.item` to indices into
/// `obj_init.fields`.
fn check_obj_init(
    comp: &metadata::CompDef,
    obj_init: &sem::ObjInit,
    diag: &mut Diag,
) -> Vec<Vec<usize>> {
    let mut initers = vec![Vec::new(); comp.items.len()];

    for (init_field_i, init_field) in obj_init.fields.iter().enumerate() {
        let item_i = comp.items.iter().position(|item| {
            item.field()
                .filter(|f| f.ident == init_field.ident.sym)
                .is_some()
        });

        let init_field_span = init_field.ident.span.map(|span| SpanLabel {
            span,
            label: None,
            style: SpanStyle::Primary,
        });

        if let Some(item_i) = item_i {
            if let Some(field) = comp.items[item_i].field() {
                if init_field.field_ty != field.field_ty {
                    diag.emit(&[Diagnostic {
                        level: Level::Error,
                        message: format!(
                            "Field type mismatch; the field `{}` is of type `{}`",
                            field.field_ty, init_field.field_ty
                        ),
                        code: None,
                        spans: init_field_span.into_iter().collect(),
                    }]);
                }

                initers[item_i].push(init_field_i);
            } else {
                diag.emit(&[Diagnostic {
                    level: Level::Error,
                    message: format!("`{}::{}` is not a field", comp.name(), init_field.ident.sym),
                    code: None,
                    spans: init_field_span.into_iter().collect(),
                }]);
            }
        } else {
            diag.emit(&[Diagnostic {
                level: Level::Error,
                message: format!(
                    "Component `{}` does not have a field named `{}`",
                    comp.name(),
                    init_field.ident.sym
                ),
                code: None,
                spans: init_field_span.into_iter().collect(),
            }]);
        }
    }

    // Report excessive or lack of initialization
    for (item, initers) in comp.items.iter().zip(initers.iter()) {
        let field = if let Some(x) = item.field() {
            x
        } else {
            continue;
        };

        if initers.len() > 1 {
            let codemap_spans: Vec<_> = initers
                .iter()
                .filter_map(|&i| obj_init.fields[i].ident.span)
                .map(|span| SpanLabel {
                    span,
                    label: None,
                    style: SpanStyle::Primary,
                })
                .collect();

            diag.emit(&[Diagnostic {
                level: Level::Error,
                message: format!("Too many initializers for the field `{}`", item.ident()),
                code: None,
                spans: codemap_spans,
            }]);
        }

        if !field.flags.contains(metadata::FieldFlags::OPTIONAL)
            && initers.is_empty()
            && field.field_ty != metadata::FieldType::Wire
        {
            diag.emit(&[Diagnostic {
                level: Level::Error,
                message: format!("Non-optional field `{}` is not initialized", field.ident),
                code: None,
                spans: obj_init
                    .path
                    .span
                    .map(|span| SpanLabel {
                        span,
                        label: None,
                        style: SpanStyle::Primary,
                    })
                    .into_iter()
                    .collect(),
            }]);
        }
    }

    initers
}

/// Generate an expression that instantiates a componen and evaluates to the
/// component's type.
///
/// `initer_map` is a multi-map from indices into `comp.item` to indices into
/// `obj_init.fields`, returned by `check_obj_init`, and may include errors
/// reported by `check_obj_init`.
fn gen_obj_init(
    comp: &metadata::CompDef,
    obj_init: &sem::ObjInit,
    analysis: &analysis::Analysis,
    ctx: &Ctx,
    item_meta2sem_map: &[usize],
    input_gen: &mut impl evalgen::FuncInputGen,
    initer_map: &[Vec<usize>],
    out: &mut String,
) {
    if comp.flags.contains(metadata::CompFlags::SIMPLE_BUILDER) {
        // Simple builder API
        let tmp_var = TempVar("built_component");
        writeln!(out, "{{").unwrap();
        writeln!(
            out,
            "    let {} = {}::new(",
            tmp_var,
            CompTy(&obj_init.path)
        )
        .unwrap();
        for (item, initers) in comp.items.iter().zip(initer_map.iter()) {
            let field = if let Some(x) = item.field() {
                x
            } else {
                continue;
            };

            // `const` is passed to `new`
            if field.field_ty == metadata::FieldType::Const
                && field.accessors.set.is_some()
                && initers.len() > 0
            {
                let obj_field = &obj_init.fields[initers[0]];
                evalgen::gen_func_eval(
                    &obj_field.value,
                    analysis,
                    ctx,
                    item_meta2sem_map,
                    input_gen,
                    out,
                );
                writeln!(out, "    ,").unwrap();
            }
        }
        writeln!(out, "    );").unwrap();

        for obj_field in obj_init
            .fields
            .iter()
            .filter(|f| f.field_ty == metadata::FieldType::Prop)
        {
            // `prop` is set through a setter method
            write!(
                out,
                "    {}.{}(",
                tmp_var,
                SetterMethod(&obj_field.ident.sym)
            )
            .unwrap();
            evalgen::gen_func_eval(
                &obj_field.value,
                analysis,
                ctx,
                item_meta2sem_map,
                input_gen,
                out,
            );
            writeln!(out, ");").unwrap();
        }

        writeln!(out, "    {}", tmp_var).unwrap();
        write!(out, "}}").unwrap();
    } else {
        // Standard builder API
        writeln!(out, "{}::new()", CompBuilderTy(&obj_init.path)).unwrap();
        for obj_field in obj_init.fields.iter() {
            write!(
                out,
                "    .{meth}(",
                meth = FactorySetterForField(&obj_field.ident.sym),
            )
            .unwrap();
            evalgen::gen_func_eval(
                &obj_field.value,
                analysis,
                ctx,
                item_meta2sem_map,
                input_gen,
                out,
            );
            writeln!(out, ")").unwrap();
        }
        write!(out, "    .build()").unwrap();
    }
}

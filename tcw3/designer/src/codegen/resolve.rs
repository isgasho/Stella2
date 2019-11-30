use codemap_diagnostic::{Diagnostic, Level, SpanLabel, SpanStyle};
use std::collections::HashMap;
use syn::{
    punctuated::Punctuated, spanned::Spanned, Ident, ItemUse, Path, PathArguments, PathSegment,
    Token, UseTree,
};

use super::{
    diag::Diag,
    parser::{span_to_codemap, Comp, File, Item},
    visit_mut,
};

/// Replace all `Path`s in the given AST with absolute paths.
pub fn resolve_paths(file: &mut File, codemap_file: &codemap::File, diag: &mut Diag) {
    let mut alias_map = HashMap::new();
    for item in file.items.iter() {
        if let Item::Use(u) = item {
            process_use(&mut alias_map, codemap_file, diag, u);
        }
    }

    // Report duplicate imports
    for (ident, aliases) in alias_map.iter() {
        if aliases.len() > 1 {
            diag.emit(&[Diagnostic {
                level: Level::Error,
                message: format!("`{}` is imported for multiple times", ident),
                code: None,
                spans: aliases
                    .iter()
                    .filter_map(|a| a.span)
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

    struct PathResolver<'a> {
        codemap_file: &'a codemap::File,
        diag: &'a mut Diag,
        alias_map: &'a HashMap<Ident, Vec<Alias>>,
    }

    impl syn::visit_mut::VisitMut for PathResolver<'_> {
        fn visit_attribute_mut(&mut self, _: &mut syn::Attribute) {}
        fn visit_item_use_mut(&mut self, _: &mut syn::ItemUse) {}

        fn visit_path_mut(&mut self, i: &mut Path) {
            if i.leading_colon.take().is_some() {
                // The path is already rooted, no need to resolve
                return;
            }

            let mut applied_map_list: Vec<(&Ident, &Alias)> = Vec::new();
            let path_span = span_to_codemap(i.span(), self.codemap_file);

            loop {
                let first_ident = &i.segments.first().unwrap().ident;

                if applied_map_list.iter().any(|(i, _)| *i == first_ident) {
                    // Detected a cycle
                    let mut spans: Vec<_> = path_span
                        .map(|span| SpanLabel {
                            span,
                            label: Some("while resolving this".to_string()),
                            style: SpanStyle::Primary,
                        })
                        .into_iter()
                        .collect();

                    spans.extend(
                        applied_map_list
                            .iter()
                            .zip(1..)
                            .filter_map(|((_, alias), k)| {
                                alias.span.map(|span| SpanLabel {
                                    span,
                                    label: Some(format!("({})", k)),
                                    style: SpanStyle::Secondary,
                                })
                            }),
                    );

                    self.diag.emit(&[Diagnostic {
                        level: Level::Error,
                        message: "Detected a cycle while resolving a path".to_string(),
                        code: None,
                        spans,
                    }]);
                    break;
                }

                if let Some((ident, aliases)) = self.alias_map.get_key_value(&first_ident) {
                    let alias = &aliases[0];

                    // Leave breadcrumbs to detect a cycle
                    applied_map_list.push((ident, alias));

                    // e.g., `a<T>::b::c` is mapped by `use self::f::g as a;`.
                    let mut new_path = Path {
                        leading_colon: None,
                        segments: Punctuated::new(),
                    };
                    let alias_rooted =
                        alias.path.segments.first().unwrap().ident.to_string() != "self";
                    let alias_start_i = if alias_rooted { 0 } else { 1 };

                    // Push `f::g`
                    for k in alias_start_i..alias.path.segments.len() {
                        new_path.segments.push(alias.path.segments[k].clone());
                    }

                    // Attach `<T>` to the last component, `g`
                    let head = new_path.segments.last_mut().unwrap();
                    head.arguments = i.segments.first().unwrap().arguments.clone();

                    // Append `::b::c` to finally get `f::g<T>::b::c`, which is
                    // resolved again because the map is not rooted (i.e.,
                    // starts with `self::`)
                    for k in 1..i.segments.len() {
                        new_path.segments.push(i.segments[k].clone());
                    }

                    *i = new_path;
                    if alias_rooted {
                        break;
                    }
                } else {
                    let spans = vec![
                        span_to_codemap(first_ident.span(), self.codemap_file).map(|span| {
                            SpanLabel {
                                span,
                                label: None,
                                style: SpanStyle::Primary,
                            }
                        }),
                        path_span.map(|span| SpanLabel {
                            span,
                            label: Some("referenced from here".to_string()),
                            style: SpanStyle::Primary,
                        }),
                    ]
                    .into_iter()
                    .filter_map(|x| x)
                    .collect();

                    self.diag.emit(&[Diagnostic {
                        level: Level::Error,
                        message: format!("Could not resolve `{}`", first_ident),
                        code: None,
                        spans,
                    }]);
                    break;
                }
            }
        }
    }

    impl visit_mut::TcwdlVisitMut for PathResolver<'_> {
        fn visit_comp_mut(&mut self, i: &mut Comp) {
            // Ignore `i.attrs`
            // Ignore `i.path` because it contains the path of the component
            // we want to *define*.
            i.items.iter_mut().for_each(|i| self.visit_comp_item_mut(i));
        }
    }

    visit_mut::visit_file_mut(
        &mut PathResolver {
            codemap_file,
            diag,
            alias_map: &alias_map,
        },
        file,
    );
}

struct Alias {
    path: Path,
    span: Option<codemap::Span>,
}

fn process_use(
    out_aliases: &mut HashMap<Ident, Vec<Alias>>,
    codemap_file: &codemap::File,
    diag: &mut Diag,
    item: &ItemUse,
) {
    if let Some(colon) = &item.leading_colon {
        diag.emit(&[Diagnostic {
            level: Level::Error,
            message: "Leading colon is not supported".to_string(),
            code: None,
            spans: span_to_codemap(colon.span(), codemap_file)
                .map(|span| SpanLabel {
                    span,
                    label: None,
                    style: SpanStyle::Primary,
                })
                .into_iter()
                .collect(),
        }]);
    }

    let mut empty_path = Path {
        leading_colon: None,
        segments: Punctuated::new(),
    };

    process_use_tree(
        &mut empty_path,
        &item.tree,
        codemap_file,
        diag,
        &mut |ident, alias| {
            out_aliases.entry(ident).or_default().push(alias);
        },
    );
}

fn process_use_tree(
    path: &mut Path,
    use_tree: &UseTree,
    codemap_file: &codemap::File,
    diag: &mut Diag,
    f: &mut impl FnMut(Ident, Alias),
) {
    match use_tree {
        UseTree::Path(t) => {
            path.segments.push(PathSegment {
                ident: t.ident.clone(),
                arguments: PathArguments::None,
            });
            process_use_tree(path, &t.tree, codemap_file, diag, f);
            path.segments.pop();
        }
        UseTree::Name(t) => {
            let rename = if t.ident.to_string() == "self" {
                if let Some(last) = path.segments.last().cloned() {
                    last.ident.clone()
                } else {
                    // This case is not supported. The error is reported during
                    // the recursive call to `process_use_tree`
                    t.ident.clone()
                }
            } else {
                t.ident.clone()
            };

            process_use_tree(
                path,
                &UseTree::Rename(syn::UseRename {
                    ident: t.ident.clone(),
                    as_token: Token![as](proc_macro2::Span::call_site()),
                    rename,
                }),
                codemap_file,
                diag,
                f,
            );
        }
        UseTree::Rename(t) => {
            let mut path = path.clone();
            if t.ident.to_string() == "self" {
                if path.segments.is_empty() {
                    diag.emit(&[Diagnostic {
                        level: Level::Error,
                        message: "Importing `self` is not allowed".to_string(),
                        code: None,
                        spans: span_to_codemap(t.ident.span(), codemap_file)
                            .map(|span| SpanLabel {
                                span,
                                label: None,
                                style: SpanStyle::Primary,
                            })
                            .into_iter()
                            .collect(),
                    }]);
                }
            } else {
                path.segments.push(PathSegment {
                    ident: t.ident.clone(),
                    arguments: PathArguments::None,
                });
            }

            f(
                t.rename.clone(),
                Alias {
                    path,
                    span: span_to_codemap(t.rename.span(), codemap_file),
                },
            );
        }
        UseTree::Glob(t) => {
            diag.emit(&[Diagnostic {
                level: Level::Error,
                message: "`*` is not supported".to_string(),
                code: None,
                spans: span_to_codemap(t.star_token.span(), codemap_file)
                    .map(|span| SpanLabel {
                        span,
                        label: None,
                        style: SpanStyle::Primary,
                    })
                    .into_iter()
                    .collect(),
            }]);
        }
        UseTree::Group(t) => {
            for item in t.items.iter() {
                process_use_tree(path, item, codemap_file, diag, f);
            }
        }
    }
}

use crate::ast::*;
use crate::parser::parse;
use crate::std_icons::{STD_ICONS_EXPORTS, STD_ICONS_SOURCE};
use crate::std_material::{STD_MATERIAL_EXPORTS, STD_MATERIAL_SOURCE};
use crate::validator::{BUILTIN_WIDGETS, validate_file_with_symbols};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

/// Virtual built-in module specifier check (`"std/widgets.cui"`, `"theme/tokens.cui"`, etc.).
pub fn is_virtual_module(path: &str) -> bool {
    matches!(
        path,
        "std/widgets.cui"
            | "std/widgets"
            | "theme/tokens.cui"
            | "theme/tokens"
            | "std/material.cui"
            | "std/material"
            | "std/icons.cui"
            | "std/icons"
    )
}

fn virtual_exports(path: &str) -> HashSet<&'static str> {
    match path {
        "std/widgets.cui" | "std/widgets" => BUILTIN_WIDGETS.iter().copied().collect(),
        "theme/tokens.cui" | "theme/tokens" => {
            ["Theme", "ThemeMode", "ThemeOverride", "ThemeTokens"]
                .into_iter()
                .collect()
        }
        "std/material.cui" | "std/material" => STD_MATERIAL_EXPORTS.iter().copied().collect(),
        "std/icons.cui" | "std/icons" => STD_ICONS_EXPORTS.iter().copied().collect(),
        _ => HashSet::new(),
    }
}

pub fn virtual_module_source(path: &str) -> Option<&'static str> {
    match path {
        "std/material.cui" | "std/material" => Some(STD_MATERIAL_SOURCE),
        "std/icons.cui" | "std/icons" => Some(STD_ICONS_SOURCE),
        _ => None,
    }
}

/// Abstract `.cui` module source loader (filesystem, in-memory VFS, tests, LSP overlay).
pub trait ModuleLoader {
    fn load(&self, path: &Path) -> std::io::Result<String>;
}

/// Standard filesystem loader.
pub struct FsModuleLoader;

impl ModuleLoader for FsModuleLoader {
    fn load(&self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
}

/// In-memory module map loader useful for virtual filesystems, WASM, and unit tests.
#[derive(Default)]
pub struct MemoryModuleLoader {
    files: HashMap<PathBuf, String>,
}

impl MemoryModuleLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl AsRef<Path>, source: impl Into<String>) {
        self.files
            .insert(normalize_lexical_path(path.as_ref()), source.into());
    }
}

impl ModuleLoader for MemoryModuleLoader {
    fn load(&self, path: &Path) -> std::io::Result<String> {
        let norm = normalize_lexical_path(path);
        self.files.get(&norm).cloned().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("virtual module not found: {}", norm.display()),
            )
        })
    }
}

/// Fully resolved & merged `.cui` compilation unit ready for validation, AOT codegen, or hot-reload.
#[derive(Debug, Clone)]
pub struct ResolvedCuiUnit {
    pub merged: CuiFile,
    pub entry_file: CuiFile,
    pub entry_path: Option<PathBuf>,
    pub imported_paths: Vec<PathBuf>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Resolve and merge a `.cui` file and all transitive local `.cui` imports from disk.
pub fn resolve_file(path: impl AsRef<Path>) -> ResolvedCuiUnit {
    let entry_path = normalize_lexical_path(path.as_ref());
    let loader = FsModuleLoader;
    match loader.load(&entry_path) {
        Ok(src) => resolve_source_internal(&src, &entry_path, Some(&loader), true),
        Err(err) => ResolvedCuiUnit {
            merged: CuiFile {
                imports: Vec::new(),
                structs: Vec::new(),
                enums: Vec::new(),
                components: Vec::new(),
            },
            entry_file: CuiFile {
                imports: Vec::new(),
                structs: Vec::new(),
                enums: Vec::new(),
                components: Vec::new(),
            },
            entry_path: Some(entry_path.clone()),
            imported_paths: Vec::new(),
            diagnostics: vec![Diagnostic {
                span: Span::default(),
                severity: Severity::Error,
                message: format!("failed reading {}: {err}", entry_path.display()),
            }],
        },
    }
}

/// Resolve `.cui` source text against an optional base directory on disk.
pub fn resolve_source(source: &str, base_dir: Option<&Path>) -> ResolvedCuiUnit {
    let entry_path = match base_dir {
        Some(dir) => normalize_lexical_path(&dir.join("__entry__.cui")),
        None => PathBuf::from("__entry__.cui"),
    };
    let loader = FsModuleLoader;
    resolve_source_internal(
        source,
        &entry_path,
        base_dir.map(|_| &loader as &dyn ModuleLoader),
        false,
    )
}

/// Resolve `.cui` source text with a custom [`ModuleLoader`].
pub fn resolve_source_with_loader(
    source: &str,
    entry_virtual_path: impl AsRef<Path>,
    loader: &dyn ModuleLoader,
) -> ResolvedCuiUnit {
    let norm = normalize_lexical_path(entry_virtual_path.as_ref());
    resolve_source_internal(source, &norm, Some(loader), true)
}

fn resolve_source_internal(
    source: &str,
    entry_path: &Path,
    loader: Option<&dyn ModuleLoader>,
    has_real_entry_path: bool,
) -> ResolvedCuiUnit {
    let parsed = parse(source);
    let mut diagnostics = parsed.diagnostics;

    let mut state = ResolveState {
        visiting: vec![entry_path.to_path_buf()],
        visited: HashSet::new(),
        imported_paths: Vec::new(),
        merged_structs: Vec::new(),
        merged_enums: Vec::new(),
        merged_components: Vec::new(),
        seen_structs: HashSet::new(),
        seen_enums: HashSet::new(),
        seen_components: HashSet::new(),
        diagnostics: Vec::new(),
    };
    state.visited.insert(entry_path.to_path_buf());

    let mut extra_known_symbols = HashSet::new();
    let base_dir = entry_path.parent().unwrap_or_else(|| Path::new("."));

    for imp in &parsed.file.imports {
        process_import(imp, base_dir, loader, &mut state, &mut extra_known_symbols);
    }

    // Merge entry declarations after dependencies, always overriding colliding imported names
    // while keeping entry declarations ordered last (so `components.last()` targets entry file).
    for st in &parsed.file.structs {
        state
            .merged_structs
            .retain(|existing| existing.name != st.name);
        state.seen_structs.insert(st.name.clone());
        state.merged_structs.push(st.clone());
    }
    for en in &parsed.file.enums {
        state
            .merged_enums
            .retain(|existing| existing.name != en.name);
        state.seen_enums.insert(en.name.clone());
        state.merged_enums.push(en.clone());
    }
    for comp in &parsed.file.components {
        state
            .merged_components
            .retain(|existing| existing.name != comp.name);
        state.seen_components.insert(comp.name.clone());
        state.merged_components.push(comp.clone());
    }

    let mut merged = CuiFile {
        imports: parsed.file.imports.clone(),
        structs: state.merged_structs,
        enums: state.merged_enums,
        components: state.merged_components,
    };

    diagnostics.extend(state.diagnostics);
    diagnostics.extend(validate_file_with_symbols(
        &parsed.file,
        &merged,
        &extra_known_symbols,
    ));

    crate::passes::run_optimization_passes(&mut merged);

    ResolvedCuiUnit {
        merged,
        entry_file: parsed.file,
        entry_path: has_real_entry_path.then(|| entry_path.to_path_buf()),
        imported_paths: state.imported_paths,
        diagnostics,
    }
}

struct ResolveState {
    visiting: Vec<PathBuf>,
    visited: HashSet<PathBuf>,
    imported_paths: Vec<PathBuf>,
    merged_structs: Vec<StructDecl>,
    merged_enums: Vec<EnumDecl>,
    merged_components: Vec<ComponentDecl>,
    seen_structs: HashSet<String>,
    seen_enums: HashSet<String>,
    seen_components: HashSet<String>,
    diagnostics: Vec<Diagnostic>,
}

fn process_import(
    imp: &ImportDecl,
    base_dir: &Path,
    loader: Option<&dyn ModuleLoader>,
    state: &mut ResolveState,
    extra_known_symbols: &mut HashSet<String>,
) {
    if is_virtual_module(&imp.from_path) {
        let valid = virtual_exports(&imp.from_path);
        for sym in &imp.symbols {
            if !valid.contains(sym.as_str()) {
                state.diagnostics.push(Diagnostic {
                    span: imp.span,
                    severity: Severity::Warning,
                    message: format!(
                        "symbol '{}' is not a standard export of '{}'",
                        sym, imp.from_path
                    ),
                });
            }
            extra_known_symbols.insert(sym.clone());
        }
        if let Some(v_src) = virtual_module_source(&imp.from_path) {
            let parsed_v = parse(v_src);
            for d in parsed_v.diagnostics {
                state.diagnostics.push(Diagnostic {
                    span: imp.span,
                    severity: d.severity,
                    message: format!("in embedded module '{}': {}", imp.from_path, d.message),
                });
            }
            for st in parsed_v.file.structs {
                if !state.seen_structs.contains(&st.name) {
                    state.seen_structs.insert(st.name.clone());
                    state.merged_structs.push(st);
                }
            }
            for en in parsed_v.file.enums {
                if !state.seen_enums.contains(&en.name) {
                    state.seen_enums.insert(en.name.clone());
                    state.merged_enums.push(en);
                }
            }
            for comp in parsed_v.file.components {
                if (imp.symbols.is_empty() || imp.symbols.iter().any(|s| s == &comp.name))
                    && !state.seen_components.contains(&comp.name)
                {
                    state.seen_components.insert(comp.name.clone());
                    state.merged_components.push(comp);
                }
            }
        }
        return;
    }

    let Some(loader) = loader else {
        // String-only API without base directory: preserve imported names for inline validation.
        for sym in &imp.symbols {
            extra_known_symbols.insert(sym.clone());
        }
        return;
    };

    let target = normalize_lexical_path(&base_dir.join(&imp.from_path));
    if state.visiting.contains(&target) {
        state.diagnostics.push(Diagnostic {
            span: imp.span,
            severity: Severity::Error,
            message: format!("circular import detected: '{}'", imp.from_path),
        });
        return;
    }

    let sub_src = match loader.load(&target) {
        Ok(s) => s,
        Err(err) => {
            state.diagnostics.push(Diagnostic {
                span: imp.span,
                severity: Severity::Error,
                message: format!("cannot resolve import '{}': {err}", imp.from_path),
            });
            return;
        }
    };

    let sub_parsed = parse(&sub_src);
    state.diagnostics.extend(sub_parsed.diagnostics);

    // Validate requested symbols exist and are exported by target module
    for sym in &imp.symbols {
        let exported = sub_parsed
            .file
            .components
            .iter()
            .any(|c| c.name == *sym && c.exported)
            || sub_parsed
                .file
                .structs
                .iter()
                .any(|s| s.name == *sym && s.exported)
            || sub_parsed
                .file
                .enums
                .iter()
                .any(|e| e.name == *sym && e.exported);

        if exported {
            extra_known_symbols.insert(sym.clone());
            continue;
        }

        let exists_private = sub_parsed.file.components.iter().any(|c| c.name == *sym)
            || sub_parsed.file.structs.iter().any(|s| s.name == *sym)
            || sub_parsed.file.enums.iter().any(|e| e.name == *sym);

        if exists_private {
            state.diagnostics.push(Diagnostic {
                span: imp.span,
                severity: Severity::Error,
                message: format!(
                    "symbol '{}' in '{}' must be marked 'export' to be imported",
                    sym, imp.from_path
                ),
            });
        } else {
            state.diagnostics.push(Diagnostic {
                span: imp.span,
                severity: Severity::Error,
                message: format!("module '{}' does not export '{}'", imp.from_path, sym),
            });
        }
    }

    if !state.visited.insert(target.clone()) {
        return;
    }
    state.imported_paths.push(target.clone());
    state.visiting.push(target.clone());

    let sub_dir = target.parent().unwrap_or_else(|| Path::new("."));
    let mut sub_known = HashSet::new();
    for nested_imp in &sub_parsed.file.imports {
        process_import(nested_imp, sub_dir, Some(loader), state, &mut sub_known);
    }

    for st in &sub_parsed.file.structs {
        if state.seen_structs.insert(st.name.clone()) {
            state.merged_structs.push(st.clone());
        }
    }
    for en in &sub_parsed.file.enums {
        if state.seen_enums.insert(en.name.clone()) {
            state.merged_enums.push(en.clone());
        }
    }
    for comp in &sub_parsed.file.components {
        if state.seen_components.insert(comp.name.clone()) {
            state.merged_components.push(comp.clone());
        }
    }

    state.visiting.pop();
}

pub fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut stack: Vec<Component<'_>> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(Component::Normal(_)) = stack.last() {
                    stack.pop();
                } else if !matches!(stack.last(), Some(Component::RootDir)) {
                    stack.push(comp);
                }
            }
            _ => stack.push(comp),
        }
    }
    stack.into_iter().collect()
}

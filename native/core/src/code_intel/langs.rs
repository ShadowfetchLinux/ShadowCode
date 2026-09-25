//! Tree-sitter grammars and queries for the code index.
//!
//! Each language has a definition query (captures `@name` on the defining
//! identifier) and a reference query (captures `@id` on every identifier-like
//! node). Definitions become rows in `symbols`; identifier counts become rows
//! in `refs`, which the repo map ranks as a def/ref graph.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
    C,
    Cpp,
    Java,
}

pub const ALL: [Lang; 9] = [
    Lang::Rust,
    Lang::TypeScript,
    Lang::Tsx,
    Lang::JavaScript,
    Lang::Python,
    Lang::Go,
    Lang::C,
    Lang::Cpp,
    Lang::Java,
];

pub fn lang_for(path: &Path) -> Option<Lang> {
    match path.extension().and_then(|e| e.to_str())? {
        "rs" => Some(Lang::Rust),
        "ts" | "mts" | "cts" => Some(Lang::TypeScript),
        "tsx" => Some(Lang::Tsx),
        "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
        "py" | "pyi" => Some(Lang::Python),
        "go" => Some(Lang::Go),
        "c" => Some(Lang::C),
        // Headers parse as C++: its grammar accepts ordinary C declarations.
        "h" | "hh" | "hpp" | "hxx" | "cc" | "cpp" | "cxx" | "c++" => Some(Lang::Cpp),
        "java" => Some(Lang::Java),
        _ => None,
    }
}

/// Files without a grammar that are still worth full-text search.
pub fn searchable_text(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.ends_with(".lock")
        || name.ends_with(".min.js")
        || name.ends_with(".map")
        || matches!(
            name,
            "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "npm-shrinkwrap.json"
        )
    {
        return false;
    }
    if matches!(
        name,
        "Makefile" | "Dockerfile" | "CMakeLists.txt" | "Justfile" | "Rakefile" | "Gemfile"
    ) {
        return true;
    }
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default(),
        "md" | "markdown"
            | "rst"
            | "txt"
            | "toml"
            | "yaml"
            | "yml"
            | "json"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "css"
            | "scss"
            | "less"
            | "html"
            | "vue"
            | "svelte"
            | "sql"
            | "lua"
            | "rb"
            | "php"
            | "cs"
            | "kt"
            | "kts"
            | "swift"
            | "scala"
            | "zig"
            | "ex"
            | "exs"
            | "erl"
            | "hs"
            | "ml"
            | "mli"
            | "r"
            | "dart"
            | "proto"
            | "graphql"
            | "gradle"
            | "cmake"
            | "nix"
            | "tf"
            | "el"
            | "clj"
            | "jl"
            | "pl"
            | "pm"
    )
}

pub fn indexable(path: &Path) -> bool {
    lang_for(path).is_some() || searchable_text(path)
}

pub fn name(lang: Lang) -> &'static str {
    match lang {
        Lang::Rust => "rust",
        Lang::TypeScript => "typescript",
        Lang::Tsx => "tsx",
        Lang::JavaScript => "javascript",
        Lang::Python => "python",
        Lang::Go => "go",
        Lang::C => "c",
        Lang::Cpp => "cpp",
        Lang::Java => "java",
    }
}

pub fn language(lang: Lang) -> tree_sitter::Language {
    match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        // The TSX grammar is a superset that also reads plain JavaScript/JSX.
        Lang::Tsx | Lang::JavaScript => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::Python => tree_sitter_python::LANGUAGE.into(),
        Lang::Go => tree_sitter_go::LANGUAGE.into(),
        Lang::C => tree_sitter_c::LANGUAGE.into(),
        Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        Lang::Java => tree_sitter_java::LANGUAGE.into(),
    }
}

const C_DEFINITIONS: &str = r#"
    (function_definition declarator: (function_declarator declarator: (identifier) @name))
    (function_definition declarator: (pointer_declarator declarator: (function_declarator declarator: (identifier) @name)))
    (declaration declarator: (function_declarator declarator: (identifier) @name))
    (struct_specifier name: (type_identifier) @name body: (field_declaration_list))
    (union_specifier name: (type_identifier) @name body: (field_declaration_list))
    (enum_specifier name: (type_identifier) @name body: (enumerator_list))
    (type_definition declarator: (type_identifier) @name)
    (preproc_def name: (identifier) @name)
    (preproc_function_def name: (identifier) @name)
"#;

pub fn definition_query(lang: Lang) -> String {
    match lang {
        Lang::Rust => r#"
            (function_item name: (identifier) @name)
            (function_signature_item name: (identifier) @name)
            (struct_item name: (type_identifier) @name)
            (enum_item name: (type_identifier) @name)
            (union_item name: (type_identifier) @name)
            (trait_item name: (type_identifier) @name)
            (impl_item type: (type_identifier) @name)
            (mod_item name: (identifier) @name)
            (const_item name: (identifier) @name)
            (static_item name: (identifier) @name)
            (type_item name: (type_identifier) @name)
            (macro_definition name: (identifier) @name)
        "#
        .into(),
        Lang::TypeScript | Lang::Tsx => r#"
            (function_declaration name: (identifier) @name)
            (class_declaration name: (type_identifier) @name)
            (abstract_class_declaration name: (type_identifier) @name)
            (interface_declaration name: (type_identifier) @name)
            (type_alias_declaration name: (type_identifier) @name)
            (enum_declaration name: (identifier) @name)
            (lexical_declaration (variable_declarator name: (identifier) @name))
            (method_definition name: (property_identifier) @name)
        "#
        .into(),
        Lang::JavaScript => r#"
            (function_declaration name: (identifier) @name)
            (class_declaration name: (type_identifier) @name)
            (lexical_declaration (variable_declarator name: (identifier) @name))
            (variable_declaration (variable_declarator name: (identifier) @name))
            (method_definition name: (property_identifier) @name)
        "#
        .into(),
        Lang::Python => r#"
            (function_definition name: (identifier) @name)
            (class_definition name: (identifier) @name)
            (module (expression_statement (assignment left: (identifier) @name)))
        "#
        .into(),
        Lang::Go => r#"
            (function_declaration name: (identifier) @name)
            (method_declaration name: (field_identifier) @name)
            (type_spec name: (type_identifier) @name)
            (source_file (const_declaration (const_spec name: (identifier) @name)))
            (source_file (var_declaration (var_spec name: (identifier) @name)))
        "#
        .into(),
        Lang::C => C_DEFINITIONS.into(),
        Lang::Cpp => format!(
            "{C_DEFINITIONS}{}",
            r#"
            (function_definition declarator: (function_declarator declarator: (field_identifier) @name))
            (function_definition declarator: (function_declarator declarator: (qualified_identifier name: (identifier) @name)))
            (function_definition declarator: (function_declarator declarator: (qualified_identifier name: (qualified_identifier name: (identifier) @name))))
            (field_declaration declarator: (function_declarator declarator: (field_identifier) @name))
            (class_specifier name: (type_identifier) @name body: (field_declaration_list))
            (namespace_definition name: (namespace_identifier) @name)
            (alias_declaration name: (type_identifier) @name)
        "#
        ),
        Lang::Java => r#"
            (class_declaration name: (identifier) @name)
            (interface_declaration name: (identifier) @name)
            (enum_declaration name: (identifier) @name)
            (record_declaration name: (identifier) @name)
            (annotation_type_declaration name: (identifier) @name)
            (method_declaration name: (identifier) @name)
            (constructor_declaration name: (identifier) @name)
        "#
        .into(),
    }
}

pub fn reference_query(lang: Lang) -> &'static str {
    match lang {
        Lang::Rust => "(identifier) @id (type_identifier) @id (field_identifier) @id",
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => {
            "(identifier) @id (type_identifier) @id (property_identifier) @id"
        }
        Lang::Python => "(identifier) @id",
        Lang::Go | Lang::C => "(identifier) @id (type_identifier) @id (field_identifier) @id",
        Lang::Cpp => {
            "(identifier) @id (type_identifier) @id (field_identifier) @id (namespace_identifier) @id"
        }
        Lang::Java => "(identifier) @id (type_identifier) @id",
    }
}

/// Node kinds that stand for a whole definition. The signature and kind of a
/// captured name come from the nearest such ancestor (or the direct parent).
pub fn definition_kinds(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Rust | Lang::TypeScript | Lang::Tsx | Lang::JavaScript => &[],
        Lang::Python => &["function_definition", "class_definition", "assignment"],
        Lang::Go => &[
            "function_declaration",
            "method_declaration",
            "type_spec",
            "const_spec",
            "var_spec",
        ],
        Lang::C | Lang::Cpp => &[
            "function_definition",
            "declaration",
            "field_declaration",
            "struct_specifier",
            "union_specifier",
            "enum_specifier",
            "class_specifier",
            "type_definition",
            "alias_declaration",
            "namespace_definition",
            "preproc_def",
            "preproc_function_def",
        ],
        Lang::Java => &[
            "class_declaration",
            "interface_declaration",
            "enum_declaration",
            "record_declaration",
            "annotation_type_declaration",
            "method_declaration",
            "constructor_declaration",
        ],
    }
}

/// Call-site shapes for "callers of": (call node kind, field holding the callee).
pub const CALL_KINDS: &[(&str, &str)] = &[
    ("call_expression", "function"),
    ("call", "function"),
    ("method_invocation", "name"),
];

/// Wrappers between an identifier and the call's callee field
/// (`a::b()`, `x.y()`, `pkg.F()`, `ns::f<T>()`).
pub const CALLEE_WRAPPERS: &[&str] = &[
    "scoped_identifier",
    "field_expression",
    "member_expression",
    "generic_function",
    "parenthesized_expression",
    "attribute",
    "selector_expression",
    "qualified_identifier",
    "template_function",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_query_compiles_for_its_grammar() {
        for lang in ALL {
            let language = language(lang);
            tree_sitter::Query::new(&language, &definition_query(lang))
                .unwrap_or_else(|e| panic!("{lang:?} definitions: {e}"));
            tree_sitter::Query::new(&language, reference_query(lang))
                .unwrap_or_else(|e| panic!("{lang:?} references: {e}"));
        }
    }

    #[test]
    fn maps_extensions() {
        assert_eq!(lang_for(Path::new("a/b.py")), Some(Lang::Python));
        assert_eq!(lang_for(Path::new("x.go")), Some(Lang::Go));
        assert_eq!(lang_for(Path::new("x.h")), Some(Lang::Cpp));
        assert_eq!(lang_for(Path::new("x.c")), Some(Lang::C));
        assert_eq!(lang_for(Path::new("X.java")), Some(Lang::Java));
        assert!(searchable_text(Path::new("README.md")));
        assert!(!searchable_text(Path::new("Cargo.lock")));
        assert!(!indexable(Path::new("image.png")));
    }
}

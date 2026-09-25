//! Which language server handles which file, where to find it, and how to
//! start it without letting it run project code or reach the network.
//!
//! Lookup order per language: a `code_intel.servers` entry in the user's
//! config.yaml; the ShadowCode-managed npm prefix (TypeScript, Python); the
//! PATH plus a few usual install folders (`~/.cargo/bin`, `~/go/bin`,
//! `~/.local/bin`) that desktop launchers often leave off PATH.
//!
//! Hardening (a server runs whenever the agent edits a file, without an
//! approval of its own):
//! - rust-analyzer: build scripts, proc macros and `cargo check` are off, so
//!   no project code is compiled or run; cargo stays offline.
//! - typescript-language-server: always uses a TypeScript installed next to
//!   the server, never `node_modules/typescript` from the project, and
//!   automatic type acquisition (npm downloads) is off.
//! - gopls: `GOTOOLCHAIN=local`, `GOPROXY=off` (no toolchain or module downloads).
//! - clangd: no background index (it would write `.cache/` into the project).
//! - pyright: analyzes open files only.
use crate::code_intel::{CodeIntelConfig, ServerOverride};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ServerLang {
    Rust,
    TypeScript,
    Python,
    Go,
    C,
}

pub const ALL: [ServerLang; 5] = [
    ServerLang::Rust,
    ServerLang::TypeScript,
    ServerLang::Python,
    ServerLang::Go,
    ServerLang::C,
];

impl ServerLang {
    /// The key used in `code_intel.servers` and in status output.
    pub fn key(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::Python => "python",
            Self::Go => "go",
            Self::C => "c",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::TypeScript => "TypeScript / JavaScript",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::C => "C / C++",
        }
    }
    /// The managed install that provides this language, if ShadowCode offers one.
    pub fn managed_package(self) -> Option<&'static str> {
        match self {
            Self::TypeScript => Some("typescript"),
            Self::Python => Some("python"),
            _ => None,
        }
    }
    pub fn install_hint(self) -> &'static str {
        match self {
            Self::Rust => "Install with: rustup component add rust-analyzer",
            Self::TypeScript => "Install from Settings › Code intelligence, or: npm install -g typescript-language-server typescript",
            Self::Python => "Install from Settings › Code intelligence, or: npm install -g pyright (or pip install basedpyright)",
            Self::Go => "Install with: go install golang.org/x/tools/gopls@latest",
            Self::C => "Install clangd from your distribution (for example: sudo apt install clangd)",
        }
    }
}

/// The server language and LSP `languageId` for a file.
pub fn for_path(path: &Path) -> Option<(ServerLang, &'static str)> {
    let ext = path.extension()?.to_str()?;
    Some(match ext {
        "rs" => (ServerLang::Rust, "rust"),
        "ts" | "mts" | "cts" => (ServerLang::TypeScript, "typescript"),
        "tsx" => (ServerLang::TypeScript, "typescriptreact"),
        "js" | "mjs" | "cjs" => (ServerLang::TypeScript, "javascript"),
        "jsx" => (ServerLang::TypeScript, "javascriptreact"),
        "py" | "pyi" => (ServerLang::Python, "python"),
        "go" => (ServerLang::Go, "go"),
        "c" | "h" => (ServerLang::C, "c"),
        "cc" | "cpp" | "cxx" | "c++" | "hh" | "hpp" | "hxx" => (ServerLang::C, "cpp"),
        _ => return None,
    })
}

struct Candidate {
    program: &'static str,
    args: &'static [&'static str],
}

fn candidates(lang: ServerLang) -> &'static [Candidate] {
    match lang {
        ServerLang::Rust => &[Candidate {
            program: "rust-analyzer",
            args: &[],
        }],
        ServerLang::TypeScript => &[Candidate {
            program: "typescript-language-server",
            args: &["--stdio"],
        }],
        ServerLang::Python => &[
            Candidate {
                program: "basedpyright-langserver",
                args: &["--stdio"],
            },
            Candidate {
                program: "pyright-langserver",
                args: &["--stdio"],
            },
        ],
        ServerLang::Go => &[Candidate {
            program: "gopls",
            args: &[],
        }],
        ServerLang::C => &[Candidate {
            program: "clangd",
            args: &[
                "--background-index=false",
                "--header-insertion=never",
                "--log=error",
                "--pch-storage=memory",
            ],
        }],
    }
}

/// A resolved way to start one server.
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub lang: ServerLang,
    /// Display name, e.g. `pyright-langserver`.
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// `config`, `managed` or `path`.
    pub source: &'static str,
    pub init_options: Value,
    /// Answers to `workspace/configuration`, by section.
    pub settings: Value,
    pub env: Vec<(String, String)>,
}

/// Usual per-user install folders that GUI sessions often omit from PATH.
pub fn extra_dirs() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    vec![
        home.join(".cargo/bin"),
        home.join("go/bin"),
        home.join(".local/bin"),
        home.join(".local/node/bin"),
    ]
}

/// PATH lookup, then the usual per-user folders.
pub fn find_program(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        return is_executable(&path).then_some(path);
    }
    crate::sandbox::which(name).or_else(|| {
        extra_dirs()
            .into_iter()
            .map(|dir| dir.join(name))
            .find(|p| is_executable(p))
    })
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    metadata.is_file()
}

/// The managed npm prefix for one package set (`typescript`, `python`).
pub fn managed_prefix(managed: &Path, package: &str) -> PathBuf {
    managed.join(package)
}

fn managed_program(managed: Option<&Path>, lang: ServerLang, program: &str) -> Option<PathBuf> {
    let package = lang.managed_package()?;
    let path = managed_prefix(managed?, package)
        .join("node_modules/.bin")
        .join(program);
    is_executable(&path).then_some(path)
}

/// `typescript/lib` shipped beside a typescript-language-server install
/// (managed prefix, global npm prefix, or the server's own node_modules).
pub fn typescript_lib_for(server: &Path) -> Option<PathBuf> {
    let real = server.canonicalize().ok()?;
    let mut roots = Vec::new();
    for ancestor in real.ancestors() {
        if ancestor.file_name().is_some_and(|n| n == "node_modules") {
            roots.push(ancestor.to_owned());
        }
        if ancestor
            .file_name()
            .is_some_and(|n| n == "typescript-language-server")
        {
            roots.push(ancestor.join("node_modules"));
        }
    }
    // A bin symlink that did not resolve into node_modules: `<prefix>/bin/x`
    // next to `<prefix>/lib/node_modules`.
    if let Some(prefix) = server.parent().and_then(Path::parent) {
        roots.push(prefix.join("lib/node_modules"));
        roots.push(prefix.join("node_modules"));
    }
    roots
        .into_iter()
        .map(|root| root.join("typescript/lib"))
        .find(|lib| lib.join("tsserver.js").is_file())
}

fn settings_for(lang: ServerLang) -> (Value, Value, Vec<(String, String)>) {
    match lang {
        ServerLang::Rust => {
            let options = json!({
                "cargo": {"buildScripts": {"enable": false}, "autoreload": true},
                "procMacro": {"enable": false},
                "checkOnSave": false,
                "check": {"workspace": false},
                "cachePriming": {"enable": false},
                "diagnostics": {"enable": true},
            });
            (
                options.clone(),
                json!({"rust-analyzer": options}),
                vec![
                    ("CARGO_NET_OFFLINE".into(), "true".into()),
                    ("RUSTUP_AUTO_INSTALL".into(), "0".into()),
                ],
            )
        }
        ServerLang::TypeScript => (
            json!({
                "disableAutomaticTypingAcquisition": true,
                "hostInfo": "ShadowCode",
            }),
            json!({}),
            Vec::new(),
        ),
        ServerLang::Python => {
            let analysis = json!({"diagnosticMode": "openFilesOnly", "autoSearchPaths": true});
            (
                json!({}),
                json!({
                    "python": {"analysis": analysis},
                    "python.analysis": analysis,
                    "basedpyright": {"analysis": analysis},
                    "basedpyright.analysis": analysis,
                    "pyright": {},
                }),
                Vec::new(),
            )
        }
        ServerLang::Go => (
            json!({}),
            json!({"gopls": {}}),
            vec![
                ("GOTOOLCHAIN".into(), "local".into()),
                ("GOPROXY".into(), "off".into()),
            ],
        ),
        ServerLang::C => (json!({}), json!({}), Vec::new()),
    }
}

/// Why a language has no usable server (for status and tool notes).
#[derive(Clone, Debug, PartialEq)]
pub enum Missing {
    NotInstalled,
    /// typescript-language-server was found without a TypeScript beside it.
    NoTypeScript(PathBuf),
    /// The configured command does not exist.
    BadOverride(String),
}

pub fn resolve(
    lang: ServerLang,
    config: &CodeIntelConfig,
    managed: Option<&Path>,
) -> Result<Launch, Missing> {
    let (mut init_options, settings, env) = settings_for(lang);
    let (name, program, args, source) = if let Some(ServerOverride { command, args }) =
        config.servers.get(lang.key())
    {
        let program = find_program(command).ok_or_else(|| Missing::BadOverride(command.clone()))?;
        let name = Path::new(command)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| command.clone());
        (name, program, args.clone(), "config")
    } else {
        let found = candidates(lang).iter().find_map(|candidate| {
            managed_program(managed, lang, candidate.program)
                .map(|p| (candidate, p, "managed"))
                .or_else(|| find_program(candidate.program).map(|p| (candidate, p, "path")))
        });
        let Some((candidate, program, source)) = found else {
            return Err(Missing::NotInstalled);
        };
        (
            candidate.program.to_owned(),
            program,
            candidate.args.iter().map(|a| a.to_string()).collect(),
            source,
        )
    };
    if lang == ServerLang::TypeScript {
        // Never let the project's own node_modules supply the TypeScript
        // server code that runs on every edit.
        let lib =
            typescript_lib_for(&program).ok_or_else(|| Missing::NoTypeScript(program.clone()))?;
        init_options["tsserver"] = json!({"path": lib.join("tsserver.js")});
    }
    Ok(Launch {
        lang,
        name,
        program,
        args,
        source,
        init_options,
        settings,
        env,
    })
}

pub fn missing_note(lang: ServerLang, missing: &Missing) -> String {
    match missing {
        Missing::NotInstalled => format!("No {} language server found. {}", lang.label(), lang.install_hint()),
        Missing::NoTypeScript(server) => format!(
            "{} has no TypeScript installed beside it, and ShadowCode does not run the project's own copy. Install the managed TypeScript server from Settings › Code intelligence.",
            server.display()
        ),
        Missing::BadOverride(command) => format!(
            "The configured {} server command was not found: {command}",
            lang.key()
        ),
    }
}

/// The environment a server gets: a short allowlist plus hardening values.
pub fn environment(launch: &Launch) -> Vec<(String, std::ffi::OsString)> {
    let mut env: Vec<(String, std::ffi::OsString)> = Vec::new();
    for name in [
        "HOME",
        "USER",
        "LANG",
        "LC_ALL",
        "TMPDIR",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
        "XDG_RUNTIME_DIR",
        "VIRTUAL_ENV",
        "CONDA_PREFIX",
        "PYTHONPATH",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "GOPATH",
        "GOROOT",
        "GOMODCACHE",
        "GOCACHE",
        "GOFLAGS",
        "JAVA_HOME",
    ] {
        if let Some(value) = std::env::var_os(name) {
            env.push((name.into(), value));
        }
    }
    // PATH gains the program's own folder (node for npm bins lives beside
    // it in a managed Node install) and the usual per-user folders.
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(dir) = launch.program.parent() {
        dirs.push(dir.to_owned());
    }
    if let Some(node) = find_program("node").and_then(|n| n.parent().map(Path::to_owned)) {
        dirs.push(node);
    }
    dirs.extend(extra_dirs());
    if let Ok(path) = std::env::join_paths(dirs) {
        env.push(("PATH".into(), path));
    }
    for (key, value) in &launch.env {
        env.push((key.clone(), value.into()));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_files_to_servers() {
        assert_eq!(
            for_path(Path::new("a.rs")),
            Some((ServerLang::Rust, "rust"))
        );
        assert_eq!(
            for_path(Path::new("a.tsx")),
            Some((ServerLang::TypeScript, "typescriptreact"))
        );
        assert_eq!(
            for_path(Path::new("x/y.py")),
            Some((ServerLang::Python, "python"))
        );
        assert_eq!(for_path(Path::new("m.hpp")), Some((ServerLang::C, "cpp")));
        assert_eq!(for_path(Path::new("README.md")), None);
    }

    #[test]
    fn overrides_win_and_bad_overrides_are_reported() {
        let mut config = CodeIntelConfig::default();
        config.servers.insert(
            "python".into(),
            ServerOverride {
                command: "/bin/sh".into(),
                args: vec!["fake.sh".into()],
            },
        );
        let launch = resolve(ServerLang::Python, &config, None).unwrap();
        assert_eq!(launch.source, "config");
        assert_eq!(launch.program, PathBuf::from("/bin/sh"));
        assert_eq!(launch.args, ["fake.sh"]);
        assert_eq!(
            launch.settings["python.analysis"]["diagnosticMode"],
            "openFilesOnly"
        );
        config.servers.insert(
            "go".into(),
            ServerOverride {
                command: "/nonexistent/gopls".into(),
                args: vec![],
            },
        );
        assert_eq!(
            resolve(ServerLang::Go, &config, None),
            Err(Missing::BadOverride("/nonexistent/gopls".into()))
        );
    }

    #[test]
    fn rust_analyzer_never_builds_or_runs_project_code() {
        let (options, settings, env) = settings_for(ServerLang::Rust);
        assert_eq!(options["cargo"]["buildScripts"]["enable"], false);
        assert_eq!(options["procMacro"]["enable"], false);
        assert_eq!(options["checkOnSave"], false);
        assert_eq!(settings["rust-analyzer"], options);
        assert!(env.contains(&("CARGO_NET_OFFLINE".into(), "true".into())));
    }

    #[test]
    fn typescript_needs_its_own_typescript() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("typescript");
        let bin = prefix.join("node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        let cli = prefix.join("node_modules/typescript-language-server/lib/cli.mjs");
        std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
        std::fs::write(&cli, "#!/usr/bin/env node\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::os::unix::fs::symlink(&cli, bin.join("typescript-language-server")).unwrap();
        }
        let config = CodeIntelConfig::default();
        assert!(matches!(
            resolve(ServerLang::TypeScript, &config, Some(dir.path())),
            Err(Missing::NoTypeScript(_))
        ));
        let lib = prefix.join("node_modules/typescript/lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("tsserver.js"), "").unwrap();
        let launch = resolve(ServerLang::TypeScript, &config, Some(dir.path())).unwrap();
        assert_eq!(launch.source, "managed");
        assert_eq!(
            launch.init_options["tsserver"]["path"],
            json!(lib.canonicalize().unwrap().join("tsserver.js"))
        );
        assert_eq!(
            launch.init_options["disableAutomaticTypingAcquisition"],
            true
        );
    }
}

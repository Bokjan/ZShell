//! The TypeScript types of what the frontend and the backend exchange, generated from the Rust
//! types into `src/ipc/bindings.ts`: `cargo test` fails while that file is out of date, and
//! rewrites it when run with `UPDATE_BINDINGS=1`.
//!
//! Every type reachable from the roots below is declared, under its `#[ts(rename)]` name.
//! Error codes become the `ErrorCode` union, from the `errors.*` keys of the English catalog.
//! The commands become `Commands`, with the arguments each takes and what it returns, read
//! from their signatures in `commands/` (see [`tests::commands`]): the frontend's `call` checks
//! both.
//! A few constants both sides need (the encodings offered, the defaults of a new session) are
//! written from the backend's definitions too, so that only one side defines them.
//! `Error` is serialized by hand, so its TypeScript type (`CommandError`) comes from a struct
//! that mirrors it (see `error.rs`).
//!
//! ts-rs rather than specta: specta 2.0 is still a release candidate, its
//! `skip_serializing_if` needs separate input and output types, and every u64 / usize field
//! needs an annotation.

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::collections::BTreeMap;
    use std::path::Path;

    use ts_rs::{Config, TypeVisitor, TS};

    const HEADER: &str = "// Generated from the Rust types and constants by `cargo test` (src-tauri/src/bindings.rs); do not edit.\n\
        // After changing them, run `UPDATE_BINDINGS=1 cargo test bindings` in src-tauri.\n\
        \n\
        import type { Channel } from \"@tauri-apps/api/core\";\n";

    /// Declarations by TypeScript name, with the Rust type each came from (its id, and its
    /// path, which commands are matched against).
    struct Collect<'a> {
        cfg: &'a Config,
        declared: BTreeMap<String, (TypeId, String)>,
        paths: Vec<(Vec<String>, String)>,
    }

    impl TypeVisitor for Collect<'_> {
        fn visit<T: TS + 'static + ?Sized>(&mut self) {
            // Primitives and wrappers (`Option`, `Vec`) declare nothing, but what they contain
            // may.
            if T::output_path().is_some() {
                let name = T::ident(self.cfg);
                if let Some((id, _)) = self.declared.get(&name) {
                    assert_eq!(*id, TypeId::of::<T::WithoutGenerics>(), "two types are named {name}; rename one with #[ts(rename)]");
                    return;
                }
                let decl = format!("{}export {}", T::docs().unwrap_or_default(), T::decl(self.cfg));
                let path = std::any::type_name::<T::WithoutGenerics>();
                let path = path.split('<').next().unwrap().split("::").skip(1).map(str::to_owned).collect();
                self.paths.push((path, name.clone()));
                self.declared.insert(name, (TypeId::of::<T::WithoutGenerics>(), decl));
            }
            T::visit_dependencies(self);
            T::visit_generics(self);
        }
    }

    fn error_codes() -> Vec<String> {
        fn walk(prefix: &str, value: &serde_json::Value, out: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(map) => {
                    for (key, child) in map {
                        walk(&if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") }, child, out);
                    }
                }
                _ => out.push(prefix.to_owned()),
            }
        }
        let catalog: serde_json::Value = serde_json::from_str(include_str!("../locales/en.json")).unwrap();
        let mut codes = Vec::new();
        walk("", &catalog["errors"], &mut codes);
        codes.sort();
        codes
    }

    /// A JavaScript literal for `value`. Object keys are sorted, whatever order serde_json
    /// keeps them in.
    fn literal(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Object(map) => {
                let mut fields: Vec<String> = map.iter().map(|(key, value)| format!("{key}: {}", literal(value))).collect();
                fields.sort();
                format!("{{ {} }}", fields.join(", "))
            }
            serde_json::Value::Array(items) => format!("[{}]", items.iter().map(literal).collect::<Vec<_>>().join(", ")),
            _ => value.to_string(),
        }
    }

    /// The events the backend sends, by name, with the type of each one's payload.
    fn events_map(cfg: &Config) -> String {
        let mut events = crate::events::declarations(cfg);
        events.sort();
        let fields: Vec<String> = events.iter().map(|(name, payload)| format!("\n  \"{name}\": {payload};")).collect();
        format!("\n/** The events the backend sends the pages, by name, with their payloads (see `events.rs`). */\nexport type Events = {{{}\n}};\n", fields.concat())
    }

    /// The commands `lib.rs` registers.
    fn registered() -> Vec<String> {
        let lib = include_str!("lib.rs");
        let start = lib.find("generate_handler![").expect("lib.rs registers the commands");
        let end = start + lib[start..].find(']').unwrap();
        let mut names: Vec<String> = lib[start..end]
            .split(',')
            .filter_map(|path| path.trim().rsplit("::").next())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        let registered = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), registered, "a command is registered twice");
        names
    }

    /// A command's signature, as `commands/` declares it.
    struct Signature {
        name: String,
        /// Where it is declared: `crate::commands::<file>`.
        module: Vec<String>,
        /// The names `use` brings into its file, with the paths they stand for.
        uses: BTreeMap<String, Vec<String>>,
        /// The arguments the frontend passes, by their names in Rust.
        args: Vec<(String, syn::Type)>,
        /// What Tauri gives it (the calling webview, the managed state), by type name.
        injected: Vec<String>,
        output: Option<syn::Type>,
    }

    /// What Tauri passes a command by itself, rather than the frontend.
    const INJECTED: &[&str] = &["AppHandle", "State", "Webview", "WebviewWindow", "Window"];

    /// The commands declared in `commands/`, read from their source.
    fn commands() -> Vec<Signature> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands");
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let file = path.file_stem().unwrap().to_str().unwrap().to_owned();
            let module = match file.as_str() {
                "mod" => vec!["crate".into(), "commands".into()],
                _ => vec!["crate".into(), "commands".into(), file],
            };
            let source = syn::parse_file(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut uses = BTreeMap::new();
            for item in &source.items {
                if let syn::Item::Use(item) = item {
                    collect_uses(&item.tree, Vec::new(), &module, &mut uses);
                }
            }
            for item in &source.items {
                let syn::Item::Fn(function) = item else { continue };
                let command = function.attrs.iter().any(|attr| {
                    let segments: Vec<String> = attr.path().segments.iter().map(|s| s.ident.to_string()).collect();
                    segments == ["tauri", "command"]
                });
                if !command {
                    continue;
                }
                let (mut args, mut injected) = (Vec::new(), Vec::new());
                for input in &function.sig.inputs {
                    let syn::FnArg::Typed(input) = input else { continue };
                    let syn::Pat::Ident(name) = &*input.pat else { panic!("{}: an argument without a name", function.sig.ident) };
                    match last_ident(&input.ty) {
                        Some(ident) if INJECTED.contains(&ident.as_str()) => injected.push(ident),
                        _ => args.push((name.ident.to_string(), (*input.ty).clone())),
                    }
                }
                let output = match &function.sig.output {
                    syn::ReturnType::Default => None,
                    syn::ReturnType::Type(_, ty) => Some((**ty).clone()),
                };
                found.push(Signature {
                    name: function.sig.ident.to_string(),
                    module: module.clone(),
                    uses: uses.clone(),
                    args,
                    injected,
                    output,
                });
            }
        }
        found.sort_by(|a, b| a.name.cmp(&b.name));
        found
    }

    /// Adds the names `tree` brings into scope, under `prefix`, to `uses`, with their paths
    /// from the crate's root.
    fn collect_uses(tree: &syn::UseTree, prefix: Vec<String>, module: &[String], uses: &mut BTreeMap<String, Vec<String>>) {
        let path = |prefix: &[String], ident: &syn::Ident| {
            let mut path: Vec<String> = prefix.to_vec();
            path.push(ident.to_string());
            match path[0].as_str() {
                "super" => [&module[..module.len() - 1], &path[1..]].concat(),
                "self" => [module, &path[1..]].concat(),
                _ => path,
            }
        };
        match tree {
            syn::UseTree::Path(next) => {
                let mut prefix = prefix;
                prefix.push(next.ident.to_string());
                collect_uses(&next.tree, prefix, module, uses);
            }
            syn::UseTree::Name(name) if name.ident == "self" => {
                let full = path(&prefix[..prefix.len() - 1], &syn::Ident::new(prefix.last().unwrap(), name.ident.span()));
                uses.insert(prefix.last().unwrap().clone(), full);
            }
            syn::UseTree::Name(name) => drop(uses.insert(name.ident.to_string(), path(&prefix, &name.ident))),
            syn::UseTree::Rename(rename) => drop(uses.insert(rename.rename.to_string(), path(&prefix, &rename.ident))),
            syn::UseTree::Group(group) => group.items.iter().for_each(|tree| collect_uses(tree, prefix.clone(), module, uses)),
            syn::UseTree::Glob(_) => {}
        }
    }

    fn last_ident(ty: &syn::Type) -> Option<String> {
        match ty {
            syn::Type::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
            syn::Type::Reference(reference) => last_ident(&reference.elem),
            _ => None,
        }
    }

    /// The type arguments of the last segment of `path` (`T` in `Option<T>`).
    fn type_args(path: &syn::Path) -> Vec<&syn::Type> {
        match &path.segments.last().unwrap().arguments {
            syn::PathArguments::AngleBracketed(args) => args
                .args
                .iter()
                .filter_map(|arg| match arg {
                    syn::GenericArgument::Type(ty) => Some(ty),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `ty`, a command's argument or result in `command`, in TypeScript: the declared type it
    /// names (`known`: the declared types' paths), or the TypeScript for a standard one.
    fn typescript(ty: &syn::Type, command: &Signature, known: &[(Vec<String>, String)]) -> String {
        let path = match ty {
            syn::Type::Tuple(tuple) if tuple.elems.is_empty() => return "void".into(),
            syn::Type::Reference(reference) => return typescript(&reference.elem, command, known),
            syn::Type::Path(path) => &path.path,
            _ => panic!("{}: a type that isn't a path", command.name),
        };
        let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        let args = type_args(path);
        let arg = |i: usize| typescript(args[i], command, known);
        match segments.last().unwrap().as_str() {
            "String" | "str" | "PathBuf" => "string".into(),
            "bool" => "boolean".into(),
            "u8" | "u16" | "u32" | "u64" | "usize" | "i32" | "i64" | "f64" | "SessionId" => "number".into(),
            "Value" if segments[0] == "serde_json" => "unknown".into(),
            "Theme" if segments[0] == "tauri" => "\"light\" | \"dark\"".into(),
            "Option" => format!("{} | null", arg(0)),
            "Vec" => match arg(0) {
                item if item.contains(' ') => format!("({item})[]"),
                item => format!("{item}[]"),
            },
            "Result" => {
                assert!(segments.len() == 1, "{}: commands return `crate::error::Result`, for the frontend's `CommandError`", command.name);
                arg(0)
            }
            // Bytes without a type: the terminal's output.
            "Channel" if args.is_empty() => "Channel<ArrayBuffer>".into(),
            "Channel" => format!("Channel<{}>", arg(0)),
            _ => declared(&segments, command, known),
        }
    }

    /// The TypeScript name of the declared type that `segments`, a path in `command`'s file,
    /// names: the one whose Rust path has its segments in order, else the only one with its
    /// name (a type re-exported from another module).
    fn declared(segments: &[String], command: &Signature, known: &[(Vec<String>, String)]) -> String {
        let path = match command.uses.get(&segments[0]) {
            Some(used) => [&used[..], &segments[1..]].concat(),
            None if segments[0] == "crate" => segments.to_vec(),
            None => [&command.module[..], segments].concat(),
        };
        let path = &path[1..];
        let name = path.last().unwrap();
        let within = |candidate: &[String]| {
            let mut rest = candidate.iter();
            path.iter().all(|segment| rest.any(|c| c == segment))
        };
        let same_name: Vec<&(Vec<String>, String)> = known.iter().filter(|(candidate, _)| candidate.last() == Some(name)).collect();
        let matching: Vec<_> = same_name.iter().filter(|(candidate, _)| within(candidate)).collect();
        match (&matching[..], &same_name[..]) {
            ([(_, ts)], _) | ([], [(_, ts)]) => ts.clone(),
            _ => panic!("{}: no declared type for `{}`; add it to the roots in `generate`", command.name, segments.join("::")),
        }
    }

    fn camel_case(name: &str) -> String {
        let mut words = name.split('_');
        let first = words.next().unwrap_or_default().to_owned();
        words.fold(first, |mut out, word| {
            let mut chars = word.chars();
            if let Some(c) = chars.next() {
                out.extend(c.to_uppercase());
                out.push_str(chars.as_str());
            }
            out
        })
    }

    /// The commands the frontend can call, with their arguments (named as Tauri expects them,
    /// in camelCase; optional ones may be left out) and results.
    fn commands_map(known: &[(Vec<String>, String)]) -> String {
        let commands = commands();
        let names: Vec<String> = commands.iter().map(|c| c.name.clone()).collect();
        assert_eq!(names, registered(), "the commands in `commands/` are those `lib.rs` registers");
        let fields: Vec<String> = commands
            .iter()
            .map(|command| {
                let args: Vec<String> = command
                    .args
                    .iter()
                    .map(|(name, ty)| {
                        let optional = if last_ident(ty).as_deref() == Some("Option") { "?" } else { "" };
                        format!("{}{optional}: {}", camel_case(name), typescript(ty, command, known))
                    })
                    .collect();
                let args = if args.is_empty() { "Record<string, never>".to_owned() } else { format!("{{ {} }}", args.join("; ")) };
                let result = command.output.as_ref().map_or("void".into(), |ty| typescript(ty, command, known));
                format!("\n  {}: {{ args: {args}; result: {result} }};", command.name)
            })
            .collect();
        format!(
            "\n/** The commands the frontend can call (see `commands/`): the arguments each takes, and what it returns. */\nexport type Commands = {{{}\n}};\n\nexport type Command = keyof Commands;\n",
            fields.concat()
        )
    }

    /// The constants written after the types: a doc comment, the declaration, the value.
    fn constants() -> String {
        use crate::*;

        fn js(value: impl serde::Serialize) -> String {
            literal(&serde_json::to_value(value).unwrap())
        }
        let constants = [
            (
                "The character encodings a session can use (WHATWG labels, as profiles store them).",
                "ENCODINGS",
                format!("{} as const", js(encoding::SUPPORTED)),
            ),
            ("The terminal type of a session that doesn't set one.", "DEFAULT_TERM_TYPE", js(model::default_term_type())),
            ("The settings of a new serial session.", "DEFAULT_SERIAL: SerialOptions", js(model::SerialOptions::default())),
            ("The quick command group that always exists, first.", "DEFAULT_GROUP", js(model::DEFAULT_GROUP)),
            ("The languages with a catalog, English first.", "LANGUAGES", format!("{} as const", js(i18n::languages()))),
            (
                "The settings before any change, which \"Restore Defaults\" goes back to.",
                "DEFAULT_SETTINGS: Settings",
                js(model::Settings::default()),
            ),
            ("The smallest terminal font size, in the settings and in sessions.", "FONT_SIZE_MIN", js(model::FONT_SIZE_MIN)),
            ("The largest terminal font size, in the settings and in sessions.", "FONT_SIZE_MAX", js(model::FONT_SIZE_MAX)),
            ("The most lines a terminal keeps above the screen.", "SCROLLBACK_MAX", js(model::SCROLLBACK_MAX)),
            (
                "Processed terminal output is acknowledged in batches of this many bytes (see `session/flow.rs`).",
                "ACK_BATCH",
                js(session::ACK_BATCH),
            ),
        ];
        constants.iter().map(|(doc, name, value)| format!("\n/** {doc} */\nexport const {name} = {value};\n")).collect()
    }

    fn generate() -> String {
        use crate::*;

        let cfg = Config::new().with_large_int("number");
        let mut collect = Collect { cfg: &cfg, declared: BTreeMap::new(), paths: Vec::new() };
        let visitor = &mut collect;
        visitor.visit::<error::Error>();
        visitor.visit::<launch::SessionSpec>();
        visitor.visit::<session::SessionSnapshot>();
        visitor.visit::<model::Folder>();
        visitor.visit::<model::Item>();
        visitor.visit::<model::Profile>();
        visitor.visit::<persist::SetAsideFile>();
        visitor.visit::<model::Proxy>();
        visitor.visit::<model::QuickCommands>();
        visitor.visit::<model::Settings>();
        visitor.visit::<ssh::known_hosts::Entry>();
        visitor.visit::<import::Candidate>();
        visitor.visit::<backup::Candidate>();
        visitor.visit::<backup::Scan>();
        visitor.visit::<vault::VaultStatus>();
        visitor.visit::<vault::Target>();
        visitor.visit::<model::AppLockSettings>();
        visitor.visit::<applock::AppLockStatus>();
        visitor.visit::<logging::LogOpen>();
        visitor.visit::<logging::LogSummary>();
        visitor.visit::<session::SessionEvent>();
        visitor.visit::<serial::PortInfo>();
        visitor.visit::<sftp::Listing>();
        visitor.visit::<sftp::transfer::Progress>();
        visitor.visit::<sftp::edit::EditEvent>();
        visitor.visit::<sftp::drag::Item>();
        visitor.visit::<sftp::drag::DragResult>();
        visitor.visit::<sftp::drag::DragEvent>();
        visitor.visit::<window::WindowInfo>();
        visitor.visit::<window::WindowContent>();
        visitor.visit::<window::WindowButton>();
        visitor.visit::<menu::Section>();
        visitor.visit::<update::Status>();
        visitor.visit::<i18n::Locales>();
        visitor.visit::<tab_drag::Rect>();
        visitor.visit::<tab_drag::Point>();
        visitor.visit::<tab_drag::DropTarget>();
        visitor.visit::<tab_drag::Dropped>();
        visitor.visit::<tab_drag::TabDragNativeEnd>();
        visitor.visit::<events::Change>();
        events::visit(visitor);

        let mut out = String::from(HEADER);
        let codes: Vec<String> = error_codes().iter().map(|code| format!("\n  | \"{code}\"")).collect();
        out.push_str(&format!("\n/** The `code` of a `CommandError`. */\nexport type ErrorCode ={};\n", codes.concat()));
        for (_, decl) in collect.declared.values() {
            out.push('\n');
            out.push_str(decl);
            out.push('\n');
        }
        out.push_str(&events_map(&cfg));
        out.push_str(&commands_map(&collect.paths));
        out.push_str(&constants());
        out
    }

    /// A command that takes a session's id answers only the session's window (see
    /// `SessionManager::check`), so it is given the calling webview to check.
    #[test]
    fn commands_taking_a_session_know_who_calls() {
        fn mentions_session(ty: &syn::Type) -> bool {
            let syn::Type::Path(path) = ty else { return false };
            path.path.segments.last().is_some_and(|s| s.ident == "SessionId") || type_args(&path.path).into_iter().any(mentions_session)
        }
        let unchecked: Vec<String> = commands()
            .into_iter()
            .filter(|command| command.args.iter().any(|(_, ty)| mentions_session(ty)))
            .filter(|command| !command.injected.iter().any(|ty| ty == "Webview" || ty == "WebviewWindow"))
            .map(|command| command.name)
            .collect();
        assert!(unchecked.is_empty(), "commands taking a session's id without the calling webview: {unchecked:?}");
    }

    #[test]
    fn bindings_are_up_to_date() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/ipc/bindings.ts");
        let generated = generate();
        if std::env::var_os("UPDATE_BINDINGS").is_some() {
            std::fs::write(&path, generated).unwrap();
            return;
        }
        // Checked out with CRLF line endings on Windows.
        let written = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        assert!(written == generated, "src/ipc/bindings.ts is out of date: run `UPDATE_BINDINGS=1 cargo test bindings` in src-tauri");
    }
}

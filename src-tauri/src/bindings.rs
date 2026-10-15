//! The TypeScript types of what the frontend and the backend exchange, generated from the Rust
//! types into `src/ipc/bindings.ts`: `cargo test` fails while that file is out of date, and
//! rewrites it when run with `UPDATE_BINDINGS=1`.
//!
//! Every type reachable from the roots below is declared, under its `#[ts(rename)]` name.
//! Error codes become the `ErrorCode` union, from the `errors.*` keys of the English catalog.
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
        // After changing them, run `UPDATE_BINDINGS=1 cargo test bindings` in src-tauri.\n";

    /// Declarations by TypeScript name, with the Rust type each came from.
    struct Collect<'a> {
        cfg: &'a Config,
        declared: BTreeMap<String, (TypeId, String)>,
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

    /// The commands the frontend can call: those `lib.rs` registers.
    fn command_names() -> String {
        let lib = include_str!("lib.rs");
        let start = lib.find("generate_handler![").expect("lib.rs registers the commands");
        let end = start + lib[start..].find(']').unwrap();
        let mut names: Vec<&str> =
            lib[start..end].split(',').filter_map(|path| path.trim().rsplit("::").next()).filter(|name| !name.is_empty()).collect();
        let registered = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), registered, "a command is registered twice");
        let names: Vec<String> = names.iter().map(|name| format!("\n  | \"{name}\"")).collect();
        format!("\n/** The commands the frontend can call (see `commands/`). */\nexport type Command ={};\n", names.concat())
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
        ];
        constants.iter().map(|(doc, name, value)| format!("\n/** {doc} */\nexport const {name} = {value};\n")).collect()
    }

    fn generate() -> String {
        use crate::*;

        let cfg = Config::new().with_large_int("number");
        let mut collect = Collect { cfg: &cfg, declared: BTreeMap::new() };
        let visitor = &mut collect;
        visitor.visit::<error::Error>();
        visitor.visit::<commands::session::SessionSpec>();
        visitor.visit::<commands::session::SessionSnapshot>();
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
        visitor.visit::<window::WindowButton>();
        visitor.visit::<menu::Section>();
        visitor.visit::<update::Status>();
        visitor.visit::<i18n::Locales>();
        visitor.visit::<tab_drag::Rect>();
        visitor.visit::<tab_drag::Point>();
        visitor.visit::<tab_drag::DropTarget>();
        visitor.visit::<tab_drag::Dropped>();
        visitor.visit::<tab_drag::TabDragNativeEnd>();
        visitor.visit::<commands::Change>();
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
        out.push_str(&command_names());
        out.push_str(&constants());
        out
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

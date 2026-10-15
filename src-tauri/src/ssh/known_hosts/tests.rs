use super::*;

const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIGTF00LNp4OdvFjVtxiUae5Ybe6KH/QRshtgH9+knbzN";
const OTHER: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
const ECDSA: &str = "AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBCLa00yW9Sf5+OVT1YXDU+6VfdRETny3PySJkcnNOtRp4F7bo7TdUnTO0drA5UYKxjy8XOteGkMO6alimShQOpY=";
/// `[127.0.0.1]:2222`, hashed by `ssh-keygen -H`.
const HASHED: &str = "|1|sXYIBsr10rhd0b/NMnyf/gj1YxQ=|/yzMUScN42HQtI9V9p2rFEu00bU=";

fn temp_file(content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zshell-known-hosts-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("known_hosts");
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn parses_entries() {
    let entry = parse_line(3, &format!("a.example,10.0.0.1 ssh-ed25519 {KEY} me@laptop")).unwrap();
    assert_eq!(entry.hosts, ["a.example", "10.0.0.1"]);
    assert_eq!(entry.algorithm, "ssh-ed25519");
    assert_eq!(entry.fingerprint.as_deref(), Some("SHA256:Qiqj/En3FON0kY6HRHfPth2y4UraxseLIxWx97Vibfs"));
    assert_eq!(entry.comment.as_deref(), Some("me@laptop"));
    assert_eq!(entry.marker, None);

    let marked = parse_line(1, &format!("@revoked\t*.example ssh-ed25519 {KEY}")).unwrap();
    assert_eq!(marked.marker.as_deref(), Some("revoked"));
    assert_eq!(marked.hosts, ["*.example"]);

    let broken = parse_line(1, "host ssh-ed25519 notbase64").unwrap();
    assert_eq!(broken.fingerprint, None);

    assert_eq!(parse_line(1, "  # comment"), None);
    assert_eq!(parse_line(1, "   "), None);
}

#[test]
fn matches_hosts_like_openssh() {
    assert!(host_matches(HASHED, "[127.0.0.1]:2222"));
    assert!(!host_matches(HASHED, "127.0.0.1"));
    assert!(host_matches("Server.Example", "server.example"));
    assert!(!host_matches("|1|garbage", "x"));
    assert!(host_matches("*.example.com", "a.b.example.com"));
    assert!(!host_matches("*.example.com", "example.com"));
    assert!(host_matches("10.0.0.?", "10.0.0.7"));
    assert!(host_matches("[*.lan]:2222", "[nas.lan]:2222"));

    let entry = parse_line(1, &format!("*.example.com,!bad.example.com ssh-ed25519 {KEY}")).unwrap();
    assert!(entry.applies_to("good.example.com"));
    assert!(!entry.applies_to("bad.example.com"));
    assert!(!entry.applies_to("other.org"));
}

#[test]
fn checks_keys_like_openssh() {
    let key = PublicKey::from_openssh(&format!("ssh-ed25519 {KEY}")).unwrap();
    // Another key of the type first, then the server's, hashed: known, as in OpenSSH.
    let path = temp_file(&format!("# c\n[127.0.0.1]:2222 ssh-ed25519 {OTHER}\n{HASHED} ssh-ed25519 {KEY}\n"));
    assert_eq!(check_at(std::slice::from_ref(&path), "127.0.0.1", 2222, &key).unwrap(), Check::Known);
    // Only another key of the type: changed, on the line counting comments.
    let path = temp_file(&format!("# c\n\n[127.0.0.1]:2222 ssh-ed25519 {OTHER}\n"));
    assert_eq!(check_at(std::slice::from_ref(&path), "127.0.0.1", 2222, &key).unwrap(), Check::Changed { path: path.clone(), line: 3 });
    assert_eq!(check_at(std::slice::from_ref(&path), "127.0.0.1", 22, &key).unwrap(), Check::Unknown);
    // A revoked key is refused even if it is also known.
    let path = temp_file(&format!("[127.0.0.1]:2222 ssh-ed25519 {KEY}\n@revoked * ssh-ed25519 {KEY}\n"));
    assert_eq!(check_at(std::slice::from_ref(&path), "127.0.0.1", 2222, &key).unwrap(), Check::Revoked { path: path.clone(), line: 2 });
    // CA lines are for certificates, wildcards apply, and host names ignore case.
    let path = temp_file(&format!("@cert-authority * ssh-ed25519 {KEY}\n*.LAN ssh-ed25519 {KEY}\n"));
    assert_eq!(check_at(std::slice::from_ref(&path), "NAS.lan", 22, &key).unwrap(), Check::Known);
    assert_eq!(check_at(std::slice::from_ref(&path), "other", 22, &key).unwrap(), Check::Unknown);
}

/// A host recorded with keys of other types only isn't a first connection, as OpenSSH
/// says; and the connection asks for the types recorded first.
#[test]
fn tells_when_other_types_of_key_are_known() {
    let key = PublicKey::from_openssh(&format!("ssh-ed25519 {KEY}")).unwrap();
    let path = temp_file(&format!("# c\n[127.0.0.1]:2222 ecdsa-sha2-nistp256 {ECDSA}\n[127.0.0.1]:2222 ecdsa-sha2-nistp256 {ECDSA}\n"));
    let other = OtherKey {
        path: path.clone(),
        line: 2,
        kind: "ECDSA".into(),
        fingerprint: "SHA256:JtxeYNtlueZkq+5MkevZVweMSrubYwvr5uRRzN3TMZc".into(),
    };
    assert_eq!(check_at(std::slice::from_ref(&path), "127.0.0.1", 2222, &key).unwrap(), Check::OtherTypesKnown(vec![other]));
    assert_eq!(check_at(std::slice::from_ref(&path), "127.0.0.1", 22, &key).unwrap(), Check::Unknown);

    let preferred = russh::Preferred::default().key.to_vec();
    let ecdsa = Algorithm::Ecdsa { curve: russh::keys::EcdsaCurve::NistP256 };
    let ordered = order_known(&preferred, std::slice::from_ref(&ecdsa));
    assert_eq!((&ordered[0], ordered.len()), (&ecdsa, preferred.len()));
    assert_eq!(ordered[1], Algorithm::Ed25519);
    // An RSA key is checked whichever hash the server signs with.
    let ordered = order_known(&preferred, &[Algorithm::Rsa { hash: None }]);
    assert!(ordered[..3].iter().all(|a| matches!(a, Algorithm::Rsa { .. })), "{ordered:?}");
    assert_eq!(order_known(&preferred, &[]), preferred);
}

#[test]
fn checks_the_other_files_too() {
    let key = PublicKey::from_openssh(&format!("ssh-ed25519 {KEY}")).unwrap();
    let user = temp_file(&format!("[127.0.0.1]:2222 ssh-ed25519 {OTHER}\n"));
    let global = temp_file(&format!("[127.0.0.1]:2222 ssh-ed25519 {KEY}\n"));
    let missing = user.with_file_name("no-such-known-hosts");
    // Known from the system's file, as in OpenSSH, although the user's has another key.
    let paths = [user.clone(), missing.clone(), global.clone()];
    assert_eq!(check_at(&paths, "127.0.0.1", 2222, &key).unwrap(), Check::Known);
    // Changed in the file that has the other key.
    assert_eq!(check_at(&paths[..2], "127.0.0.1", 2222, &key).unwrap(), Check::Changed { path: user.clone(), line: 1 });
    // Revoked in any file.
    let revoked = temp_file(&format!("\n@revoked * ssh-ed25519 {KEY}\n"));
    let paths = [global.clone(), revoked.clone()];
    assert_eq!(check_at(&paths, "127.0.0.1", 2222, &key).unwrap(), Check::Revoked { path: revoked, line: 2 });
    // A file of the system's that can't be read is skipped; the user's own is not. A
    // folder can't be read as one (`temp_dir` ends with a separator on Windows, where that
    // reads as not found).
    let directory = global.parent().unwrap().to_path_buf();
    assert_eq!(check_at(&[global.clone(), directory.clone()], "127.0.0.1", 2222, &key).unwrap(), Check::Known);
    assert!(check_at(&[directory, global], "127.0.0.1", 2222, &key).is_err());
}

#[test]
fn learns_keys_as_openssh_writes_them() {
    let key = PublicKey::from_openssh(&format!("ssh-ed25519 {KEY}")).unwrap();
    let path = temp_file("");
    learn_at(&path, "Server.LAN", 22, &key).unwrap();
    learn_at(&path, "::1", 2222, &key).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("server.lan ssh-ed25519 {KEY}\n[::1]:2222 ssh-ed25519 {KEY}\n"));
    assert_eq!(check_at(std::slice::from_ref(&path), "server.lan", 22, &key).unwrap(), Check::Known);
    // After a last line without a line break.
    let path = temp_file("# mine");
    learn_at(&path, "a", 22, &key).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("# mine\na ssh-ed25519 {KEY}\n"));
    // A host that would be read as a pattern is refused.
    for host in ["*", "a,b", "!a", "a?", "[a]", "a b", ""] {
        assert!(learn_at(&path, host, 22, &key).is_err(), "{host}");
    }
    // Where there is no folder yet.
    let path = temp_file("").with_file_name("new").join("known_hosts");
    learn_at(&path, "a", 22, &key).unwrap();
    assert!(path.is_file());
}

#[test]
fn matches_wildcards_as_openssh() {
    // `match_pattern`'s definition, in exponential time.
    fn reference(pattern: &[u8], text: &[u8]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some((b'*', rest)) => (0..=text.len()).any(|skip| reference(rest, &text[skip..])),
            Some((&c, rest)) => {
                text.split_first().is_some_and(|(&t, text_rest)| (c == b'?' || c.eq_ignore_ascii_case(&t)) && reference(rest, text_rest))
            }
        }
    }
    // Every pattern and text up to a length, from a few characters.
    fn all(alphabet: &[u8], max: usize) -> Vec<Vec<u8>> {
        let mut all = vec![Vec::new()];
        let mut last = all.clone();
        for _ in 0..max {
            last = last.iter().flat_map(|s| alphabet.iter().map(move |&c| [s.as_slice(), &[c]].concat())).collect();
            all.extend(last.iter().cloned());
        }
        all
    }
    let texts = all(b"aAb", 5);
    for pattern in all(b"ab*?", 5) {
        for text in &texts {
            assert_eq!(glob_matches(&pattern, text), reference(&pattern, text), "{pattern:?} {text:?}");
        }
    }
    // Many stars that can't match take no time to give up on.
    let pattern = format!("{}x", "*".repeat(64));
    let started = std::time::Instant::now();
    assert!(!glob_matches(pattern.as_bytes(), b"[server.lan]:2222"));
    assert!(started.elapsed() < std::time::Duration::from_millis(100));
}

#[test]
fn reads_queries_as_hosts() {
    assert_eq!(query_host_port("127.0.0.1:2222").as_deref(), Some("[127.0.0.1]:2222"));
    assert_eq!(query_host_port("[::1]:2222").as_deref(), Some("[::1]:2222"));
    assert_eq!(query_host_port("host:22").as_deref(), Some("host"));
    assert_eq!(query_host_port("::1").as_deref(), Some("::1"));
    assert_eq!(query_host_port("two words"), None);
}

#[test]
fn counts_comment_lines() {
    let path = temp_file(&format!("# comment\n\n{HASHED} ssh-ed25519 {OTHER}\n"));
    let entries = list_at(&path).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].line, 3);
}

/// A comment in another encoding doesn't stop the file from being used or edited.
#[test]
fn reads_and_edits_files_that_are_not_utf8() {
    let path = temp_file("");
    let comment = b"# \xc4\xe3\xba\xc3 (GBK)\n".as_slice();
    let content = [comment, format!("a ssh-ed25519 {KEY}\nb ssh-ed25519 {OTHER}\n").as_bytes()].concat();
    fs::write(&path, &content).unwrap();
    let key = PublicKey::from_openssh(&format!("ssh-ed25519 {KEY}")).unwrap();
    assert!(matches!(check_at(std::slice::from_ref(&path), "a", 22, &key).unwrap(), Check::Known));
    let entries = list_at(&path).unwrap();
    remove_at(&path, entries[1].line, &entries[1].text).unwrap();
    assert_eq!(fs::read(&path).unwrap(), [comment, format!("a ssh-ed25519 {KEY}\n").as_bytes()].concat());
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn removes_a_line_and_keeps_the_rest() {
    use std::io::Write;
    let content = format!("# keep\r\na ssh-ed25519 {KEY}\r\nb ssh-ed25519 {OTHER}");
    let path = temp_file(&content);
    let entries = list_at(&path).unwrap();
    assert_eq!(entries[0].text, format!("a ssh-ed25519 {KEY}"));

    // The file changed since it was listed.
    assert_eq!(remove_at(&path, 2, "b").unwrap_err().code(), "knownHosts.changed");
    assert_eq!(remove_at(&path, 9, "b").unwrap_err().code(), "knownHosts.changed");

    // A key added while it was being removed (by `ssh`, which doesn't wait for ZShell)
    // is kept: the removal fails instead.
    let append = || fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"\nc ssh-ed25519 KEY").unwrap();
    assert_eq!(remove_with(&path, 2, &entries[0].text, append).unwrap_err().code(), "knownHosts.changed");
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("{content}\nc ssh-ed25519 KEY"));
    fs::write(&path, &content).unwrap();

    remove_at(&path, 2, &entries[0].text).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("# keep\r\nb ssh-ed25519 {OTHER}"));
    assert_eq!(fs::read_to_string(path.with_file_name("known_hosts.old")).unwrap(), content);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// The copy of a private file is private too, and so is a new `~/.ssh`.
#[cfg(unix)]
#[test]
fn keeps_private_files_private() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let path = temp_file(&format!(
        "a ssh-ed25519 {KEY}
b ssh-ed25519 {OTHER}
"
    ));
    let old = path.with_file_name("known_hosts.old");
    // One left by an earlier removal, when the file was still readable by others.
    fs::write(&old, "").unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let entries = list_at(&path).unwrap();
    remove_at(&path, entries[0].line, &entries[0].text).unwrap();
    assert_eq!((mode(&path), mode(&old)), (0o600, 0o600));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();

    let key = PublicKey::from_openssh(&format!("ssh-ed25519 {KEY}")).unwrap();
    let path = temp_file("").with_file_name(".ssh").join("known_hosts");
    learn_at(&path, "a", 22, &key).unwrap();
    assert_eq!(mode(path.parent().unwrap()), 0o700);
    fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
}

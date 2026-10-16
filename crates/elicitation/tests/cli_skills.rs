//! `elicitation --version|--help`, the `skills install|uninstall|list`
//! subcommand, and the no-argument MCP stdio server, driven through the real
//! binary against temp directories.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_elicitation");
const MANIFEST_FILE: &str = ".elicitation-skills.json";
const SKILLS: [&str; 3] = ["elicitation", "elicit-interview", "elicit-to-plan"];

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn skills_root(base: &Path, target: &str) -> PathBuf {
    let dir = match target {
        "claude" => ".claude",
        "codex" => ".agents",
        other => panic!("unknown target {other}"),
    };
    base.join(dir).join("skills")
}

fn install(base: &Path, target: &str, extra: &[&str]) -> Output {
    let dir = base.to_str().expect("utf-8 temp path");
    let mut args = vec!["skills", "install", "--target", target, "--dir", dir];
    args.extend_from_slice(extra);
    run(&args)
}

#[test]
fn version_flag_prints_the_name_and_version() {
    let out = run(&["--version"]);
    assert_eq!(stdout(&out), format!("{} {}\n", "elicitation", env!("CARGO_PKG_VERSION")));
}

#[test]
fn short_version_flag_prints_the_name_and_version() {
    let out = run(&["-V"]);
    assert_eq!(stdout(&out), format!("elicitation {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_flag_prints_usage() {
    let out = run(&["--help"]);
    assert!(stdout(&out).contains("Usage:"));
}

#[test]
fn install_writes_every_skill_file() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path(), "claude", &[]);
    let missing: Vec<String> = SKILLS
        .iter()
        .filter(|name| {
            !skills_root(dir.path(), "claude")
                .join(name)
                .join("SKILL.md")
                .is_file()
        })
        .map(|name| name.to_string())
        .collect();
    assert!(missing.is_empty(), "missing skill files: {missing:?}");
}

#[test]
fn install_writes_a_manifest_beside_the_skills() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path(), "claude", &[]);
    assert!(
        skills_root(dir.path(), "claude")
            .join(MANIFEST_FILE)
            .is_file()
    );
}

#[test]
fn install_refuses_to_overwrite_a_modified_skill() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path(), "claude", &[]);
    let skill = skills_root(dir.path(), "claude")
        .join("elicitation")
        .join("SKILL.md");
    let mut edited = std::fs::read_to_string(&skill).unwrap();
    edited.push_str("\n<!-- user edit -->\n");
    std::fs::write(&skill, &edited).unwrap();

    install(dir.path(), "claude", &[]);

    assert_eq!(std::fs::read_to_string(&skill).unwrap(), edited);
}

#[test]
fn install_writes_both_targets_for_target_both() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path(), "both", &[]);
    let installed: BTreeSet<&str> = ["claude", "codex"]
        .into_iter()
        .filter(|target| {
            skills_root(dir.path(), target)
                .join("elicitation")
                .join("SKILL.md")
                .is_file()
        })
        .collect();
    assert_eq!(installed, BTreeSet::from(["claude", "codex"]));
}

#[test]
fn uninstall_removes_an_unchanged_installed_skill() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path(), "claude", &[]);
    let base = dir.path().to_str().unwrap();
    run(&["skills", "uninstall", "--target", "claude", "--dir", base]);
    let skill = skills_root(dir.path(), "claude")
        .join("elicitation")
        .join("SKILL.md");
    assert!(!skill.exists());
}

#[test]
fn list_reports_every_installed_skill_name() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path(), "claude", &[]);
    let base = dir.path().to_str().unwrap();
    let out = stdout(&run(&["skills", "list", "--dir", base]));
    let missing: Vec<&str> = SKILLS
        .iter()
        .copied()
        .filter(|name| !out.contains(name))
        .collect();
    assert!(missing.is_empty(), "list omitted {missing:?} from: {out}");
}

#[test]
fn no_arguments_serves_the_six_mcp_tools_over_stdio() {
    let mut child = Command::new(BIN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the server starts");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    let _ = tx.send(line);
                }
                Err(_) => break,
            }
        }
    });
    let read = |rx: &mpsc::Receiver<String>| -> serde_json::Value {
        let line = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("server response");
        serde_json::from_str(&line).expect("valid JSON-RPC")
    };

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2024-11-05","capabilities":{{}},"clientInfo":{{"name":"test","version":"1.0"}}}}}}"#
    )
    .unwrap();
    let _ = read(&rx);
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{}}}}"#
    )
    .unwrap();
    let list = read(&rx);
    let _ = child.kill();
    let _ = child.wait();

    let names: BTreeSet<String> = list["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    let expected: BTreeSet<String> = [
        "session.open",
        "append",
        "state.get",
        "question.next",
        "readiness.assess",
        "spec.export",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(names, expected);
}

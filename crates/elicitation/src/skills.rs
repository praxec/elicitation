//! `elicitation skills install|uninstall|list`: copies the agent skills under
//! the repository-root `skills/` directory (embedded at compile time) into the
//! directories each agent tool reads.
//!
//! Every skills root we write to gets a manifest, [`MANIFEST_FILE`], that
//! records the sha256 of each file as we wrote it and the targets installed
//! into the root. A later run uses it to tell our files from the user's:
//!
//! - a file that is not in the manifest is someone else's ("foreign, skipped");
//! - a file whose hash no longer matches the manifest was edited by the user
//!   ("modified, skipped");
//! - a file that still matches the manifest is ours to update or remove.
//!
//! `--force` overwrites foreign and modified files. Writes are atomic (a temp
//! file in the same directory, then a rename). The installer never writes MCP
//! client configuration.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/embedded_skills.rs"));

/// The manifest file written in each skills root.
pub const MANIFEST_FILE: &str = ".elicitation-skills.json";
/// The manifest schema version this binary writes and reads.
pub const MANIFEST_SCHEMA: u32 = 1;
/// Exit code for a successful run (skipped files included).
pub const EXIT_OK: u8 = 0;
/// Exit code for an IO error or a refused change.
pub const EXIT_IO: u8 = 1;
/// Exit code for a usage error.
pub const EXIT_USAGE: u8 = 2;

/// Usage text for the `skills` subcommand.
pub const USAGE: &str = "\
Usage:
  elicitation skills install   --target <claude|codex|both> (--dir <path> | --user | --project) [--dry-run] [--force]
  elicitation skills uninstall --target <claude|codex|both> (--dir <path> | --user | --project) [--dry-run]
  elicitation skills list      (--dir <path> | --user | --project)

Targets:
  claude     Claude Code   <base>/.claude/skills/
  codex      OpenAI Codex  <base>/.agents/skills/
  both       both of the above

Scope:
  --dir <path>   an explicit project directory
  --project      the current directory
  --user         your home directory

A file you edited, or one elicitation did not write, is kept and reported as
skipped; --force overwrites it. No MCP configuration is written.
";

/// Every embedded file under `skills/` as (path relative to `skills/`, contents).
pub fn embedded_skills() -> &'static [(&'static str, &'static str)] {
    EMBEDDED_SKILLS
}

/// The names of the embedded skills (directories holding a `SKILL.md`), sorted.
pub fn skill_names() -> Vec<&'static str> {
    SKILL_DESCRIPTIONS.iter().map(|(name, _)| *name).collect()
}

/// The front-matter `description` of an embedded skill (validated at build time).
pub fn skill_description(name: &str) -> Option<&'static str> {
    SKILL_DESCRIPTIONS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, d)| *d)
}

/// Lower-case hex sha256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The per-root record of what elicitation wrote.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    /// [`MANIFEST_SCHEMA`] at write time.
    pub schema: u32,
    /// The elicitation version that wrote the manifest.
    pub elicitation_version: String,
    /// The targets (`claude`, `codex`) that installed into this root.
    #[serde(default)]
    pub targets: BTreeSet<String>,
    /// Path relative to the root (`/` separators) to the sha256 of the content
    /// elicitation wrote there.
    pub files: BTreeMap<String, String>,
}

/// What happened to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Created,
    Updated,
    Unchanged,
    Modified,
    Foreign,
    Removed,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Created => "created",
            Status::Updated => "updated",
            Status::Unchanged => "unchanged",
            Status::Modified => "modified, skipped",
            Status::Foreign => "foreign, skipped",
            Status::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Claude,
    Codex,
    Both,
}

impl Target {
    fn parse(raw: &str) -> Option<Target> {
        Some(match raw {
            "claude" => Target::Claude,
            "codex" => Target::Codex,
            "both" => Target::Both,
            _ => return None,
        })
    }

    /// The concrete roots this target writes, as (skills dir, target id).
    fn roots(self, base: &Path) -> Vec<(PathBuf, &'static str)> {
        let claude = (base.join(".claude").join("skills"), "claude");
        let codex = (base.join(".agents").join("skills"), "codex");
        match self {
            Target::Claude => vec![claude],
            Target::Codex => vec![codex],
            Target::Both => vec![claude, codex],
        }
    }
}

/// A usage error (exit 2) or an IO error / refusal (exit 1).
#[derive(Debug)]
enum CliError {
    Usage(String),
    Io(String),
}

fn io_err(context: impl std::fmt::Display, err: io::Error) -> CliError {
    CliError::Io(format!("{context}: {err}"))
}

/// The user's home: `HOME`, then `USERPROFILE`.
fn home_dir() -> Result<PathBuf, CliError> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            CliError::Io(
                "cannot find your home directory: neither HOME nor USERPROFILE is set".into(),
            )
        })
}

// ---------------------------------------------------------------- filesystem

fn read_existing(path: &Path) -> Result<Option<Vec<u8>>, CliError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(io_err(path.display(), err)),
    }
}

/// Write `bytes` to `path` via a temp file in the same directory and a rename.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let dir = path
        .parent()
        .ok_or_else(|| CliError::Io(format!("{} has no parent directory", path.display())))?;
    fs::create_dir_all(dir).map_err(|e| io_err(dir.display(), e))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.elicitation-tmp-{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let result = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if let Err(err) = result {
        let _ = fs::remove_file(&tmp);
        return Err(io_err(path.display(), err));
    }
    Ok(())
}

fn prune_empty_dirs(root: &Path, path: &Path) {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        if fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

fn join_rel(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c))
}

// ---------------------------------------------------------------- manifest

fn load_manifest(root: &Path) -> Result<Option<Manifest>, CliError> {
    let path = root.join(MANIFEST_FILE);
    let Some(bytes) = read_existing(&path)? else {
        return Ok(None);
    };
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| {
        CliError::Io(format!(
            "{} is not a valid manifest ({e}); remove it to start over",
            path.display()
        ))
    })?;
    if manifest.schema > MANIFEST_SCHEMA {
        return Err(CliError::Io(format!(
            "{} was written by elicitation {} (manifest schema {}); upgrade elicitation",
            path.display(),
            manifest.elicitation_version,
            manifest.schema
        )));
    }
    Ok(Some(manifest))
}

fn save_manifest(root: &Path, manifest: &Manifest) -> Result<(), CliError> {
    let path = root.join(MANIFEST_FILE);
    if manifest.files.is_empty() && manifest.targets.is_empty() {
        if path.exists() {
            fs::remove_file(&path).map_err(|e| io_err(path.display(), e))?;
        }
        return Ok(());
    }
    let mut manifest = manifest.clone();
    manifest.schema = MANIFEST_SCHEMA;
    manifest.elicitation_version = env!("CARGO_PKG_VERSION").to_string();
    let mut json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| CliError::Io(format!("cannot encode the manifest: {e}")))?;
    json.push('\n');
    write_atomic(&path, json.as_bytes())
}

/// Decide what to do with one file. Returns (status, write, manifest hash to
/// record; `None` keeps the existing entry unchanged).
fn decide(
    current: Option<&[u8]>,
    content: &[u8],
    recorded: Option<&String>,
    force: bool,
) -> (Status, bool, Option<String>) {
    let new_hash = sha256_hex(content);
    let Some(current) = current else {
        return (Status::Created, true, Some(new_hash));
    };
    if current == content {
        return (Status::Unchanged, false, Some(new_hash));
    }
    if force || recorded == Some(&sha256_hex(current)) {
        (Status::Updated, true, Some(new_hash))
    } else if recorded.is_some() {
        (Status::Modified, false, None)
    } else {
        (Status::Foreign, false, None)
    }
}

// ---------------------------------------------------------------- report

fn print_line(out: &mut dyn io::Write, line: &str) -> Result<(), CliError> {
    writeln!(out, "{line}").map_err(|e| io_err("stdout", e))
}

// ---------------------------------------------------------------- options

enum Scope {
    User,
    Project,
    Dir(PathBuf),
}

struct Options {
    target: Option<Target>,
    scope: Option<Scope>,
    dry_run: bool,
    force: bool,
}

fn parse_options(args: &[String], allow: &[&str]) -> Result<Options, CliError> {
    let mut opts = Options {
        target: None,
        scope: None,
        dry_run: false,
        force: false,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        if !allow.contains(&flag) {
            return Err(CliError::Usage(format!("unexpected argument `{arg}`")));
        }
        let mut value = |name: &str| -> Result<String, CliError> {
            match inline.clone().or_else(|| iter.next().cloned()) {
                Some(v) if !v.is_empty() => Ok(v),
                _ => Err(CliError::Usage(format!("{name} needs a value"))),
            }
        };
        let set_scope = |opts: &mut Options, scope: Scope| -> Result<(), CliError> {
            if opts.scope.is_some() {
                return Err(CliError::Usage(
                    "give one of --dir <path>, --project or --user".into(),
                ));
            }
            opts.scope = Some(scope);
            Ok(())
        };
        if matches!(flag, "--user" | "--project" | "--force" | "--dry-run") && inline.is_some() {
            return Err(CliError::Usage(format!("{flag} takes no value")));
        }
        match flag {
            "--target" => {
                let raw = value("--target")?;
                opts.target = Some(Target::parse(&raw).ok_or_else(|| {
                    CliError::Usage(format!("unknown target `{raw}` (claude, codex, both)"))
                })?);
            }
            "--dir" => {
                let raw = value("--dir")?;
                let dir = std::path::absolute(&raw)
                    .map_err(|e| CliError::Usage(format!("--dir {raw}: {e}")))?;
                set_scope(&mut opts, Scope::Dir(dir))?;
            }
            "--project" => set_scope(&mut opts, Scope::Project)?,
            "--user" => set_scope(&mut opts, Scope::User)?,
            "--dry-run" => opts.dry_run = true,
            "--force" => opts.force = true,
            _ => unreachable!("flag is in the allow list"),
        }
    }
    Ok(opts)
}

fn base_dir(opts: &Options) -> Result<PathBuf, CliError> {
    match &opts.scope {
        Some(Scope::Dir(p)) => Ok(p.clone()),
        Some(Scope::User) => home_dir(),
        Some(Scope::Project) => std::env::current_dir().map_err(|e| io_err("current dir", e)),
        None => Err(CliError::Usage(
            "give one of --dir <path>, --project or --user".into(),
        )),
    }
}

fn require_target(opts: &Options) -> Result<Target, CliError> {
    opts.target
        .ok_or_else(|| CliError::Usage("--target <claude|codex|both> is required".into()))
}

// ---------------------------------------------------------------- commands

/// Run `elicitation skills <args>`; returns the process exit code.
pub fn run(args: &[String]) -> u8 {
    let mut stdout = io::stdout().lock();
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("install") => parse_options(
            rest,
            &[
                "--target",
                "--dir",
                "--user",
                "--project",
                "--dry-run",
                "--force",
            ],
        )
        .and_then(|o| install(&o, &mut stdout)),
        Some("uninstall") => parse_options(
            rest,
            &["--target", "--dir", "--user", "--project", "--dry-run"],
        )
        .and_then(|o| uninstall(&o, &mut stdout)),
        Some("list") => parse_options(rest, &["--dir", "--user", "--project"])
            .and_then(|o| list(&o, &mut stdout)),
        Some("--help" | "-h" | "help") => {
            let _ = stdout.write_all(USAGE.as_bytes());
            Ok(())
        }
        Some(other) => Err(CliError::Usage(format!("unknown skills command `{other}`"))),
        None => Err(CliError::Usage("missing skills command".into())),
    };
    let _ = stdout.flush();
    match result {
        Ok(()) => EXIT_OK,
        Err(CliError::Usage(msg)) => {
            eprintln!("elicitation skills: {msg}\n\n{USAGE}");
            EXIT_USAGE
        }
        Err(CliError::Io(msg)) => {
            eprintln!("elicitation skills: error: {msg}");
            EXIT_IO
        }
    }
}

fn install(opts: &Options, out: &mut dyn io::Write) -> Result<(), CliError> {
    let target = require_target(opts)?;
    let base = base_dir(opts)?;
    for (root, target_id) in target.roots(&base) {
        install_root(opts, &root, target_id, out)?;
    }
    Ok(())
}

fn install_root(
    opts: &Options,
    root: &Path,
    target_id: &str,
    out: &mut dyn io::Write,
) -> Result<(), CliError> {
    let mut manifest = load_manifest(root)?.unwrap_or_default();
    manifest.targets.insert(target_id.to_string());

    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (rel, content) in EMBEDDED_SKILLS {
        let path = join_rel(root, rel);
        let current = read_existing(&path)?;
        let recorded = manifest.files.get(*rel);
        let (status, write, hash) =
            decide(current.as_deref(), content.as_bytes(), recorded, opts.force);
        if write && !opts.dry_run {
            write_atomic(&path, content.as_bytes())?;
        }
        if let Some(hash) = hash {
            manifest.files.insert((*rel).to_string(), hash);
        }
        *counts.entry(status.label()).or_default() += 1;
        print_line(out, &format!("{}  {}", status.label(), path.display()))?;
    }

    // Files an older elicitation wrote that this one no longer ships.
    let shipped: BTreeSet<&str> = EMBEDDED_SKILLS.iter().map(|(rel, _)| *rel).collect();
    let stale: Vec<String> = manifest
        .files
        .keys()
        .filter(|rel| !shipped.contains(rel.as_str()))
        .cloned()
        .collect();
    for rel in stale {
        let path = join_rel(root, &rel);
        let recorded = manifest.files.get(&rel).cloned().unwrap_or_default();
        match read_existing(&path)? {
            None => {
                manifest.files.remove(&rel);
            }
            Some(current) if sha256_hex(&current) == recorded => {
                if !opts.dry_run {
                    fs::remove_file(&path).map_err(|e| io_err(path.display(), e))?;
                    prune_empty_dirs(root, &path);
                }
                manifest.files.remove(&rel);
                *counts.entry(Status::Removed.label()).or_default() += 1;
                print_line(
                    out,
                    &format!("{}  {}", Status::Removed.label(), path.display()),
                )?;
            }
            Some(_) => {
                *counts.entry(Status::Modified.label()).or_default() += 1;
                print_line(
                    out,
                    &format!("{}  {}", Status::Modified.label(), path.display()),
                )?;
            }
        }
    }

    if !opts.dry_run {
        save_manifest(root, &manifest)?;
    }
    let summary = counts
        .iter()
        .map(|(label, n)| format!("{n} {label}"))
        .collect::<Vec<_>>()
        .join(", ");
    print_line(
        out,
        &format!(
            "elicitation skills install: {summary} in {}",
            root.display()
        ),
    )
}

fn uninstall(opts: &Options, out: &mut dyn io::Write) -> Result<(), CliError> {
    let target = require_target(opts)?;
    let base = base_dir(opts)?;
    for (root, target_id) in target.roots(&base) {
        uninstall_root(opts, &root, target_id, out)?;
    }
    Ok(())
}

fn uninstall_root(
    opts: &Options,
    root: &Path,
    target_id: &str,
    out: &mut dyn io::Write,
) -> Result<(), CliError> {
    let Some(mut manifest) = load_manifest(root)? else {
        print_line(out, &format!("nothing installed in {}", root.display()))?;
        return Ok(());
    };
    manifest.targets.remove(target_id);
    if !manifest.targets.is_empty() {
        print_line(
            out,
            &format!(
                "kept (still used by {})  {}",
                manifest
                    .targets
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                root.display()
            ),
        )?;
        if !opts.dry_run {
            save_manifest(root, &manifest)?;
        }
        return Ok(());
    }

    let rels: Vec<String> = manifest.files.keys().cloned().collect();
    for rel in rels {
        let path = join_rel(root, &rel);
        let recorded = manifest.files.get(&rel).cloned().unwrap_or_default();
        match read_existing(&path)? {
            None => {
                manifest.files.remove(&rel);
            }
            Some(current) if sha256_hex(&current) == recorded => {
                if !opts.dry_run {
                    fs::remove_file(&path).map_err(|e| io_err(path.display(), e))?;
                    prune_empty_dirs(root, &path);
                }
                manifest.files.remove(&rel);
                print_line(
                    out,
                    &format!("{}  {}", Status::Removed.label(), path.display()),
                )?;
            }
            Some(_) => {
                print_line(
                    out,
                    &format!("{}  {}", Status::Modified.label(), path.display()),
                )?;
            }
        }
    }
    if !opts.dry_run {
        manifest.files.clear();
        save_manifest(root, &manifest)?;
    }
    Ok(())
}

fn list(opts: &Options, out: &mut dyn io::Write) -> Result<(), CliError> {
    let base = base_dir(opts)?;
    for (root, _) in Target::Both.roots(&base) {
        match load_manifest(&root)? {
            None => {
                print_line(out, &format!("not installed  {}", root.display()))?;
            }
            Some(manifest) => {
                let mut names: BTreeSet<String> = BTreeSet::new();
                for rel in manifest.files.keys() {
                    if let Some(name) = rel.split('/').next() {
                        names.insert(name.to_string());
                    }
                }
                let names: Vec<String> = names.into_iter().collect();
                print_line(out, &format!("{}: {}", root.display(), names.join(", ")))?;
            }
        }
    }
    Ok(())
}

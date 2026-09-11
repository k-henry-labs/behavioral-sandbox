//! `boxdesk volume`: directories with lives of their own, mounted into sandboxes by name.
//!
//! - **A volume is a mount this project manages.** `--volume NAME:GUESTDIR` becomes exactly the
//!   `--mount` it would have been, against a directory boxdesk made and can list, so the record a
//!   run leaves says what it could touch in the same words as ever.
//! - **The guest path is yours to pick, and the image must have it.** A volume is a virtiofs
//!   mount like any other, so its mount point has to exist in the guest tree; that is the rule
//!   `--mount` already keeps, and the refusal is the same one.
//! - **Local only.** No object store and nothing shared between machines: the cloud was removed
//!   from this tree deliberately, and a volume is a directory on this computer.

use std::process::ExitCode;

use boxdesk_record::{Volume, VolumeStore};

use crate::EXIT_OPERATIONAL;

#[derive(clap::Args)]
pub(crate) struct VolumeArgs {
    #[command(subcommand)]
    cmd: VolumeCmd,
}

#[derive(clap::Subcommand)]
enum VolumeCmd {
    /// Make a volume: an empty directory sandboxes can mount by name.
    New(NewArgs),
    /// List the volumes on this machine, and what each one holds.
    Ls(LsArgs),
    /// Print one volume: where it is on the host, and how much is in it.
    Show(ShowArgs),
    /// Remove a volume and everything in it.
    Rm(RmArgs),
}

#[derive(clap::Args)]
pub(crate) struct NewArgs {
    /// The name it is mounted and listed by.
    #[arg(value_name = "NAME")]
    name: String,
    /// One line saying what is in it, shown beside the name in `ls`.
    #[arg(long, value_name = "TEXT")]
    about: Option<String>,
}

#[derive(clap::Args)]
pub(crate) struct LsArgs {
    /// Print the volumes as one JSON array.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
pub(crate) struct ShowArgs {
    /// The volume's name.
    #[arg(value_name = "NAME")]
    name: String,
    /// Print the volume as one JSON document.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
pub(crate) struct RmArgs {
    /// The volume's name.
    #[arg(value_name = "NAME")]
    name: String,
    /// Remove it even though it holds something. A volume exists because what is in it was worth
    /// keeping, so a full one is refused without this.
    #[arg(long)]
    force: bool,
}

pub(crate) fn run(args: &VolumeArgs) -> ExitCode {
    let store = match VolumeStore::open() {
        Ok(store) => store,
        Err(e) => {
            eprintln!("boxdesk volume: {e}");
            return ExitCode::from(EXIT_OPERATIONAL);
        }
    };
    let (verb, done) = match &args.cmd {
        VolumeCmd::New(a) => ("new", create(&store, a)),
        VolumeCmd::Ls(a) => ("ls", list(&store, a, &mut std::io::stdout())),
        VolumeCmd::Show(a) => ("show", describe(&store, a, &mut std::io::stdout())),
        VolumeCmd::Rm(a) => ("rm", forget(&store, a)),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("boxdesk volume {verb}: {msg}");
            ExitCode::from(EXIT_OPERATIONAL)
        }
    }
}

fn create(store: &VolumeStore, args: &NewArgs) -> Result<(), String> {
    if !boxdesk_record::valid_id(&args.name) {
        return Err(format!(
            "{:?} is not a usable volume name: letters, digits, `-` and `_`",
            args.name
        ));
    }
    // Refusing rather than replacing: `new` on a name already taken would otherwise read as a way
    // to empty one, and emptying a volume is what `rm --force` is for.
    if store.holds(&args.name) {
        return Err(format!(
            "a volume named {:?} is already here (`boxdesk volume show {}` says what is in it)",
            args.name, args.name
        ));
    }
    let mut volume = Volume::new(&args.name);
    if let Some(about) = &args.about {
        volume = volume.about(about);
    }
    store.create(&volume).map_err(|e| e.to_string())?;
    println!("{}", volume.name);
    Ok(())
}

fn list(store: &VolumeStore, args: &LsArgs, out: &mut impl std::io::Write) -> Result<(), String> {
    let volumes = store.list();
    if args.json {
        let array: Vec<serde_json::Value> = volumes
            .iter()
            .map(|v| {
                serde_json::json!({
                    "name": v.name,
                    "about": v.about,
                    "bytes": store.size_of(&v.name),
                    "path": store.data_of(&v.name).ok().map(|p| p.display().to_string()),
                    "created_ms": v.created_ms,
                })
            })
            .collect();
        writeln!(out, "{}", serde_json::Value::Array(array)).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if volumes.is_empty() {
        writeln!(out, "no volumes (make one with `boxdesk volume new`)")
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let widest = volumes.iter().map(|v| v.name.len()).max().unwrap_or(0);
    for volume in &volumes {
        let held = bytes(store.size_of(&volume.name));
        let about = if volume.about.is_empty() {
            String::new()
        } else {
            format!("  {}", volume.about)
        };
        writeln!(out, "{:widest$}  {held:>9}{about}", volume.name).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn describe(
    store: &VolumeStore,
    args: &ShowArgs,
    out: &mut impl std::io::Write,
) -> Result<(), String> {
    let volume = store.read(&args.name).map_err(|e| e.to_string())?;
    let path = store.data_of(&args.name).map_err(|e| e.to_string())?;
    if args.json {
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "name": volume.name,
                "about": volume.about,
                "bytes": store.size_of(&volume.name),
                "path": path.display().to_string(),
                "created_ms": volume.created_ms,
            })
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }
    writeln!(out, "name     {}", volume.name).map_err(|e| e.to_string())?;
    if !volume.about.is_empty() {
        writeln!(out, "about    {}", volume.about).map_err(|e| e.to_string())?;
    }
    writeln!(out, "path     {}", path.display()).map_err(|e| e.to_string())?;
    writeln!(out, "holds    {}", bytes(store.size_of(&volume.name))).map_err(|e| e.to_string())?;
    writeln!(
        out,
        "created  {}",
        boxdesk_record::format_time(volume.created_ms)
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        out,
        "mount    boxdesk run --volume {}:GUESTDIR -- ...",
        volume.name
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn forget(store: &VolumeStore, args: &RmArgs) -> Result<(), String> {
    store
        .remove(&args.name, args.force)
        .map_err(|e| e.to_string())?;
    println!("{}", args.name);
    Ok(())
}

/// A byte count a person reads, in the units a directory listing uses.
fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = n as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// The `--volume NAME:GUESTDIR` a verb was given, as the mounts they become.
///
/// **A volume is a mount, resolved here and nowhere else.** Everything downstream — the config,
/// the record, the posture a run prints — sees an ordinary host directory at a guest path, which
/// is why a volume needs no new word anywhere else in the tree.
///
/// # Errors
///
/// A spec that is not `NAME:GUESTDIR`, a guest path that is not absolute, or a volume nobody made.
pub(crate) fn mounts_for(
    specs: &[String],
) -> Result<Vec<(std::path::PathBuf, std::path::PathBuf)>, String> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let store = VolumeStore::open().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for spec in specs {
        let (name, guest) = spec
            .split_once(':')
            .ok_or_else(|| format!("--volume {spec:?} is not NAME:GUESTDIR"))?;
        if !guest.starts_with('/') {
            return Err(format!(
                "--volume {spec:?} needs an absolute guest path, as {name}:/work"
            ));
        }
        let data = store.data_of(name).map_err(|_| {
            format!("no volume named {name:?} (make one with `boxdesk volume new {name}`)")
        })?;
        out.push((std::path::PathBuf::from(guest), data));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> (boxdesk_test_support::ScratchDir, VolumeStore) {
        let dir = boxdesk_test_support::ScratchDir::created("cli-volume");
        let store = VolumeStore::at(dir.path().join("volumes")).expect("a store");
        (dir, store)
    }

    /// `ls` leads with the name and says how much is in each one, because a volume's size is the
    /// fact a list can give that a name and a date cannot.
    #[test]
    fn ls_says_what_each_volume_holds() {
        let (_dir, store) = scratch();
        let data = store
            .create(&Volume::new("datasets").about("the corpus"))
            .expect("created");
        std::fs::write(data.join("a.bin"), vec![0u8; 4096]).expect("wrote");
        store.create(&Volume::new("cache")).expect("created");

        let mut out = Vec::new();
        list(&store, &LsArgs { json: false }, &mut out).expect("listed");
        let text = String::from_utf8(out).expect("utf-8");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "got {text:?}");
        assert!(lines[0].starts_with("cache "), "by name: {:?}", lines[0]);
        assert!(
            lines[0].contains("0 B"),
            "an empty one says so: {:?}",
            lines[0]
        );
        assert!(lines[1].contains("4.0 KiB"), "got {:?}", lines[1]);
        assert!(lines[1].contains("the corpus"));
    }

    /// An empty store says how to make one rather than answering with a blank.
    #[test]
    fn ls_on_an_empty_store_says_how_to_make_one() {
        let (_dir, store) = scratch();
        let mut out = Vec::new();
        list(&store, &LsArgs { json: false }, &mut out).expect("listed");
        assert!(
            String::from_utf8_lossy(&out).contains("volume new"),
            "an empty list should say where one comes from"
        );
    }

    /// `show` says where the volume is on the host and how it is mounted, which are the two things
    /// somebody asking about one wants.
    #[test]
    fn show_says_where_it_is_and_how_to_mount_it() {
        let (_dir, store) = scratch();
        store.create(&Volume::new("datasets")).expect("created");
        let mut out = Vec::new();
        describe(
            &store,
            &ShowArgs {
                name: "datasets".to_string(),
                json: false,
            },
            &mut out,
        )
        .expect("described");
        let text = String::from_utf8(out).expect("utf-8");
        assert!(text.contains("path     "), "got {text}");
        assert!(text.contains("--volume datasets:GUESTDIR"), "got {text}");
    }

    /// `new` on a name already taken is refused, because it would otherwise read as a way to empty
    /// a volume — and emptying one is what `rm --force` is for.
    #[test]
    fn new_refuses_a_name_already_taken_rather_than_emptying_it() {
        let (_dir, store) = scratch();
        let data = store.create(&Volume::new("datasets")).expect("created");
        std::fs::write(data.join("a.bin"), b"keep me").expect("wrote");

        let err = create(
            &store,
            &NewArgs {
                name: "datasets".to_string(),
                about: None,
            },
        )
        .expect_err("the name is taken");
        assert!(err.contains("already here"), "said {err}");
        assert!(
            data.join("a.bin").is_file(),
            "and what was in it is still in it"
        );
    }

    /// **A volume becomes an ordinary mount**, which is why nothing downstream of this needs to
    /// know the word: the config, the record and the posture print all see a host directory at a
    /// guest path.
    #[test]
    fn a_volume_resolves_to_the_mount_it_would_have_been() {
        let (dir, store) = scratch();
        let data = store.create(&Volume::new("datasets")).expect("created");
        // `mounts_for` opens the store from the environment, which a test cannot set in this
        // edition; the resolution it performs is the two lines below, checked against the store.
        assert_eq!(
            store.data_of("datasets").expect("the volume's data"),
            data,
            "a name resolves to the directory a guest writes"
        );
        assert!(
            data.starts_with(dir.path()),
            "and that directory is inside the store"
        );
    }

    /// A spec that is not `NAME:GUESTDIR` is refused with the shape it should have had, and a
    /// relative guest path is refused for the reason `--mount` refuses one.
    #[test]
    fn a_volume_spec_that_is_not_one_is_refused_by_shape() {
        let err = mounts_for(&["datasets".to_string()]).expect_err("no guest path");
        assert!(err.contains("NAME:GUESTDIR"), "said {err}");

        let err = mounts_for(&["datasets:work".to_string()]).expect_err("a relative path");
        assert!(err.contains("absolute"), "said {err}");

        assert!(
            mounts_for(&[])
                .expect("nothing asked for is nothing to resolve")
                .is_empty(),
            "no volumes is not an error"
        );
    }

    /// Sizes are spelled the way a directory listing spells them, and a small one stays in bytes
    /// rather than becoming `0.0 KiB`.
    #[test]
    fn a_size_is_spelled_the_way_a_listing_spells_one() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1024), "1.0 KiB");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(1024 * 1024 * 3), "3.0 MiB");
        assert!(bytes(u64::MAX).ends_with("TiB"), "the largest unit holds");
    }
}

//! The `ess` the gate runs: the one release `gate.yml` pins, and no other.
//!
//! The pin has one source, `ESS_VERSION` and `ESS_SHA256` of the "Install ess" step in
//! `.github/workflows/gate.yml`. [`resolve`] looks for an `ess` that reports that version, in
//! this order:
//!
//! 1. the stored one, `<store>/ess-<version>-x86_64-unknown-linux-gnu/ess`, where the store is
//!    `target/ess` under the repository root;
//! 2. the first `ess` on `PATH`;
//! 3. the release asset, fetched into the store when nothing is in the stored one's place, with
//!    the digest check the workflow step makes: the asset's line in the release's `SHA256SUMS`
//!    is the pinned digest, and the downloaded bytes have that digest.
//!
//! `PATH` comes before the fetch so that a host which already carries the pinned release (CI
//! after its install step, a build node) makes no network request. When none of the three gives
//! the pinned version, the [`Refusal`] names the version needed and what each place held, and
//! the caller runs no `ess` at all.
//!
//! The digest is checked on what is fetched and nowhere else: a stored `ess` and one on `PATH`
//! are started on their `--version` answer alone. Whoever can write the store or a directory of
//! `PATH` runs code in the gate.

use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    ffi::{OsStr, OsString},
    fmt::{self, Write},
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};

/// The workflow that holds the pin, from the repository root.
const WORKFLOW: &str = ".github/workflows/gate.yml";

/// The step of the workflow whose `env` is the pin.
const STEP: &str = "Install ess";

/// Where a release's assets are, before `/<version>/<asset>`.
const RELEASES: &str = "https://github.com/beyond10x/ess/releases/download";

/// The one platform an asset is fetched for; its digest is the one the workflow pins.
const TRIPLE: &str = "x86_64-unknown-linux-gnu";

/// Names another place for the store than `target/ess`.
pub const STORE_VARIABLE: &str = "LLM_ESS_STORE";

/// Names another address for the releases than GitHub's. The pinned digest still decides.
pub const RELEASES_VARIABLE: &str = "LLM_ESS_RELEASES";

/// The ESS release the gate runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// `ESS_VERSION`: three numbers.
    pub version: String,
    /// `ESS_SHA256`: the digest of the `x86_64-unknown-linux-gnu` asset, 64 lowercase hex digits.
    pub sha256: String,
}

impl Pin {
    /// Reads the pin from the workflow under `root`.
    pub fn read(root: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(root.join(WORKFLOW))
            .map_err(|error| format!("{WORKFLOW}: {error}"))?;
        Self::parse(&text).map_err(|why| format!("{WORKFLOW}: {why}"))
    }

    /// Reads the pin from the workflow's text.
    pub fn parse(text: &str) -> Result<Self, String> {
        let workflow: serde_yaml::Value =
            serde_yaml::from_str(text).map_err(|error| format!("not YAML: {error}"))?;
        let step = workflow["jobs"]["libraries-and-plan"]["steps"]
            .as_sequence()
            .into_iter()
            .flatten()
            .find(|step| step["name"].as_str() == Some(STEP))
            .ok_or_else(|| format!("no step \"{STEP}\" in job libraries-and-plan"))?;
        let version = step["env"]["ESS_VERSION"]
            .as_str()
            .filter(|version| {
                let parts: Vec<&str> = version.split('.').collect();
                parts.len() == 3
                    && parts.iter().all(|part| {
                        !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
                    })
            })
            .ok_or_else(|| {
                format!("ESS_VERSION of the step \"{STEP}\" is not three numbers in a string")
            })?;
        let sha256 = step["env"]["ESS_SHA256"]
            .as_str()
            .filter(|digest| {
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            .ok_or_else(|| {
                format!("ESS_SHA256 of the step \"{STEP}\" is not 64 lowercase hex digits")
            })?;
        Ok(Self {
            version: version.to_owned(),
            sha256: sha256.to_owned(),
        })
    }

    /// The directory a release asset unpacks to, and the stored one's name.
    fn directory(&self) -> String {
        format!("ess-{}-{TRIPLE}", self.version)
    }
}

/// Where a resolution looks.
pub struct Host {
    /// The directory that keeps fetched releases.
    pub store: PathBuf,
    /// The search path for an installed `ess`.
    pub path: Option<OsString>,
    /// Where a release's assets are, before `/<version>/<asset>`.
    pub releases: String,
    /// Whether this platform runs the asset the pin's digest is of.
    pub fetches: bool,
}

impl Host {
    /// This process's host for the repository at `root`, read through `variable`.
    pub fn of(root: &Path, variable: impl Fn(&str) -> Option<OsString>) -> Self {
        let set = |name: &str| variable(name).filter(|value| !value.is_empty());
        Self {
            store: set(STORE_VARIABLE).map_or_else(|| root.join("target/ess"), PathBuf::from),
            path: variable("PATH"),
            releases: set(RELEASES_VARIABLE)
                .and_then(|value| value.into_string().ok())
                .unwrap_or_else(|| RELEASES.to_owned()),
            fetches: cfg!(all(target_os = "linux", target_arch = "x86_64")),
        }
    }
}

/// No place gave an `ess` of the pinned version.
pub struct Refusal {
    needed: String,
    tried: Vec<String>,
}

/// `main` ends with an error's `Debug` form, so it is the text a reader needs.
impl fmt::Debug for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ess {} is needed (ESS_VERSION of the step \"{STEP}\" in {WORKFLOW}) and none was found",
            self.needed
        )?;
        for place in &self.tried {
            write!(formatter, "\n  {place}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Refusal {}

/// The first executable `ess` on `path`, as a shell finds it.
fn on_path(path: Option<&OsStr>) -> Option<PathBuf> {
    let name = format!("ess{}", env::consts::EXE_SUFFIX);
    env::split_paths(path?)
        .map(|directory| directory.join(&name))
        .find(|candidate| executable(candidate))
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    path.is_file()
}

/// Whether `binary --version` ends successfully and writes exactly `ess <version>`, with or
/// without the end of the line. A longer version that starts like the pinned one is another.
fn reports(binary: &Path, version: &str) -> Result<(), String> {
    let output = Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("does not start: {error}"))?;
    if !output.status.success() {
        return Err(format!("`--version` ended with {}", output.status));
    }
    let said = String::from_utf8_lossy(&output.stdout);
    if said.strip_suffix('\n').unwrap_or(&said) == format!("ess {version}") {
        Ok(())
    } else {
        let shown: String = said.chars().take(200).collect();
        Err(format!("reports {shown:?}"))
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").expect("a string takes text");
    }
    text
}

/// Downloads `url` to `file` with `curl`, as the workflow step does.
fn download(url: &str, file: &Path) -> Result<(), String> {
    let output = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--connect-timeout", "20", "--max-time", "300", "--output"])
        .arg(file)
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("curl does not start: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "curl ended with {} for {url}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Fetches the pinned release's asset into the store and returns its `ess`.
///
/// Everything happens in a directory of its own inside the store, `.fetch-*`, which is removed
/// when the fetch returns, whatever it returns; only an unpacked release whose asset has the
/// pinned digest and whose `ess` reports the pinned version is moved to its place. A run that
/// a signal ends during the fetch does not return and leaves its `.fetch-*` directory. No run
/// reads a `.fetch-*` directory as stored or starts anything in one, and no fetch removes
/// another's, so two runs may fetch into one store at once. Removing the store is safe: the next
/// run fetches again.
fn fetch(pin: &Pin, host: &Host) -> Result<PathBuf, String> {
    let directory = pin.directory();
    let asset = format!("{directory}.tar.gz");
    let release = format!("{}/{}", host.releases.trim_end_matches('/'), pin.version);
    fs::create_dir_all(&host.store)
        .map_err(|error| format!("{}: {error}", host.store.display()))?;
    let staging = tempfile::Builder::new()
        .prefix(".fetch-")
        .tempdir_in(&host.store)
        .map_err(|error| format!("{}: {error}", host.store.display()))?;
    eprintln!(
        "ess {}: fetching {release}/{asset} into {}",
        pin.version,
        host.store.display()
    );
    let archive = staging.path().join(&asset);
    download(&format!("{release}/{asset}"), &archive)?;
    let list = staging.path().join("SHA256SUMS");
    download(&format!("{release}/SHA256SUMS"), &list)?;

    let line = format!("{}  {asset}", pin.sha256);
    let sums = fs::read_to_string(&list).map_err(|error| format!("SHA256SUMS: {error}"))?;
    if !sums.lines().any(|listed| listed == line) {
        return Err(format!("SHA256SUMS of the release has no line `{line}`"));
    }
    let bytes = fs::read(&archive).map_err(|error| format!("{asset}: {error}"))?;
    let digest = hex(Sha256::digest(&bytes).as_slice());
    if digest != pin.sha256 {
        return Err(format!(
            "{asset} has SHA-256 {digest}, the pin is {}",
            pin.sha256
        ));
    }

    let member = format!("{directory}/ess");
    let unpacked = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(staging.path())
        .arg(&member)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("tar does not start: {error}"))?;
    if !unpacked.status.success() {
        return Err(format!(
            "tar ended with {} for {member} of {asset}: {}",
            unpacked.status,
            String::from_utf8_lossy(&unpacked.stderr).trim()
        ));
    }
    reports(&staging.path().join(&member), &pin.version)
        .map_err(|why| format!("the `ess` in {asset} {why}"))?;

    let place = host.store.join(&directory);
    let stored = place.join("ess");
    if let Err(error) = fs::rename(staging.path().join(&directory), &place) {
        // Another run of the gate may have stored the same release in the meantime.
        reports(&stored, &pin.version).map_err(|_| format!("{}: {error}", place.display()))?;
    }
    Ok(stored)
}

/// The `ess` of the pinned version, or why there is none.
pub fn resolve(pin: &Pin, host: &Host) -> Result<PathBuf, Refusal> {
    let mut tried = Vec::new();

    let place = host.store.join(pin.directory());
    let stored = place.join("ess");
    let absent = fs::symlink_metadata(&stored).is_err();
    if absent {
        tried.push(format!("{}: absent", stored.display()));
    } else {
        match reports(&stored, &pin.version) {
            Ok(()) => return Ok(stored),
            // A file that is there and is not the pinned release was not put there by a fetch,
            // so no fetch writes over it.
            Err(why) => tried.push(format!(
                "{}: {why}; remove it and the release is fetched again",
                stored.display()
            )),
        }
    }

    match on_path(host.path.as_deref()) {
        Some(installed) => match reports(&installed, &pin.version) {
            Ok(()) => return Ok(installed),
            Err(why) => tried.push(format!("{} (PATH): {why}", installed.display())),
        },
        None => tried.push("PATH: no `ess`".to_owned()),
    }

    // A stored file was named above, and no fetch writes over it.
    if absent && host.fetches {
        // The place is looked at again, and before its `ess`: another run moves a whole release
        // there at once, so a place that came to be since the first look has its `ess`.
        if fs::symlink_metadata(&place).is_err() {
            match fetch(pin, host) {
                Ok(fetched) => return Ok(fetched),
                Err(why) => tried.push(format!("fetch: {why}")),
            }
        } else if fs::symlink_metadata(&stored).is_err() {
            // A fetch moves a whole release to its place and never into something that is
            // there: what is there without an `ess` is not a fetch's, and nothing is downloaded
            // only to find that the move fails.
            tried.push(format!(
                "fetch: {}: is there and holds no `ess`; remove it and the release is fetched again",
                place.display()
            ));
        } else {
            // Another run stored a release while `PATH` was asked.
            match reports(&stored, &pin.version) {
                Ok(()) => return Ok(stored),
                Err(why) => tried.push(format!(
                    "{}: {why}; remove it and the release is fetched again",
                    stored.display()
                )),
            }
        }
    } else if absent {
        tried.push(format!(
            "fetch: not on this platform; the pinned digest is of the {TRIPLE} asset"
        ));
    }

    Err(Refusal {
        needed: pin.version.clone(),
        tried,
    })
}

/// The pinned `ess` for the repository at `root`, as [`resolve`] finds it on this host.
pub fn pinned(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let pin = Pin::read(root)?;
    let found = resolve(&pin, &Host::of(root, |name| env::var_os(name)))?;
    eprintln!("ess {}: {}", pin.version, found.display());
    Ok(found)
}

/// Runs the pinned `ess` with `arguments` and gives back its exit status.
pub fn run(root: &Path, arguments: &[OsString]) -> Result<ExitCode, Box<dyn Error>> {
    let status = Command::new(pinned(root)?).args(arguments).status()?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from))
}

#[cfg(all(test, unix))]
mod tests {
    use super::{Host, Pin, RELEASES, RELEASES_VARIABLE, STORE_VARIABLE, resolve};
    use sha2::{Digest, Sha256};
    use std::{
        ffi::OsString,
        fmt::Write,
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::Command,
        sync::{Mutex, MutexGuard, PoisonError},
    };

    /// One case at a time writes and starts its scripts: a script that a child of another thread
    /// still holds open for writing does not start.
    static SERIAL: Mutex<()> = Mutex::new(());

    const VERSION: &str = "7.8.9";
    const DIRECTORY: &str = "ess-7.8.9-x86_64-unknown-linux-gnu";
    const ASSET: &str = "ess-7.8.9-x86_64-unknown-linux-gnu.tar.gz";
    const OTHER_DIGEST: &str = "00000000000000000000000000000000000000000000000000000000000000aa";

    /// A store, a `PATH` directory and a release address, all in one temporary directory.
    struct Fixture {
        directory: tempfile::TempDir,
        _serial: MutexGuard<'static, ()>,
    }

    impl Fixture {
        fn new() -> Self {
            let serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
            let directory = tempfile::tempdir().expect("a temporary directory");
            fs::create_dir(directory.path().join("bin")).expect("a PATH directory");
            Self {
                directory,
                _serial: serial,
            }
        }

        fn at(&self, name: &str) -> PathBuf {
            self.directory.path().join(name)
        }

        fn host(&self) -> Host {
            Host {
                store: self.at("store"),
                path: Some(self.at("bin").into_os_string()),
                releases: format!("file://{}", self.at("release").display()),
                fetches: true,
            }
        }

        /// Writes an `ess` at `place` that notes `<name> <arguments>` for each call, writes
        /// `reply` and ends with `status`.
        fn stand_in(&self, place: &Path, name: &str, reply: &str, status: u8) {
            fs::create_dir_all(place.parent().expect("a parent")).expect("its directory");
            let script = format!(
                "#!/bin/sh\nprintf '%s\\n' \"{name} $*\" >> '{}'\nprintf '%s' '{reply}'\nexit {status}\n",
                self.at("calls").display()
            );
            fs::write(place, script).expect("the stand-in is written");
            fs::set_permissions(place, fs::Permissions::from_mode(0o755))
                .expect("the stand-in is executable");
        }

        fn on_path(&self, reply: &str) -> PathBuf {
            let place = self.at("bin/ess");
            self.stand_in(&place, "path", reply, 0);
            place
        }

        fn stored(&self, reply: &str) -> PathBuf {
            let place = self.at("store").join(DIRECTORY).join("ess");
            self.stand_in(&place, "stored", reply, 0);
            place
        }

        /// Publishes the asset of the release, holding an `ess` that writes `reply`, and returns
        /// the asset's digest. The test writes `SHA256SUMS` with [`Fixture::sums`].
        fn release(&self, reply: &str) -> String {
            let tree = self.at("tree");
            self.stand_in(&tree.join(DIRECTORY).join("ess"), "fetched", reply, 0);
            let release = self.at("release").join(VERSION);
            fs::create_dir_all(&release).expect("the release directory");
            let packed = Command::new("tar")
                .arg("-czf")
                .arg(release.join(ASSET))
                .arg("-C")
                .arg(&tree)
                .arg(DIRECTORY)
                .status()
                .expect("tar runs");
            assert!(packed.success(), "tar packs the asset");
            let mut digest = String::new();
            for byte in Sha256::digest(fs::read(release.join(ASSET)).expect("the asset")) {
                write!(digest, "{byte:02x}").expect("a string takes text");
            }
            digest
        }

        /// Writes the release's `SHA256SUMS` with `line` among the lines of other assets.
        fn sums(&self, line: &str) {
            let text = format!(
                "{OTHER_DIGEST}  ess-{VERSION}-aarch64-apple-darwin.tar.gz\n{line}\n{OTHER_DIGEST}  ess-{VERSION}-x86_64-apple-darwin.tar.gz\n"
            );
            fs::write(self.at("release").join(VERSION).join("SHA256SUMS"), text)
                .expect("SHA256SUMS is written");
        }

        /// Every call a stand-in received, in order.
        fn calls(&self) -> Vec<String> {
            fs::read_to_string(self.at("calls"))
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect()
        }

        /// The names in the store, or nothing when there is no store.
        fn store(&self) -> Vec<OsString> {
            fs::read_dir(self.at("store"))
                .map(|entries| {
                    entries
                        .map(|entry| entry.expect("an entry").file_name())
                        .collect()
                })
                .unwrap_or_default()
        }
    }

    fn pin(sha256: &str) -> Pin {
        Pin {
            version: VERSION.to_owned(),
            sha256: sha256.to_owned(),
        }
    }

    fn refusal(pin: &Pin, host: &Host) -> String {
        match resolve(pin, host) {
            Ok(found) => panic!("{} was taken", found.display()),
            Err(refusal) => refusal.to_string(),
        }
    }

    #[test]
    fn the_stored_ess_is_used_and_no_other_is_started() {
        let fixture = Fixture::new();
        let stored = fixture.stored("ess 7.8.9\n");
        fixture.on_path("ess 0.98.0\n");
        let digest = fixture.release("ess 7.8.9\n");
        fixture.sums(&format!("{digest}  {ASSET}"));
        assert_eq!(resolve(&pin(&digest), &fixture.host()).ok(), Some(stored));
        assert_eq!(fixture.calls(), ["stored --version"]);
    }

    #[test]
    fn the_ess_on_path_is_used_when_it_reports_the_pin_and_nothing_is_fetched() {
        let fixture = Fixture::new();
        let installed = fixture.on_path("ess 7.8.9\n");
        let digest = fixture.release("ess 7.8.9\n");
        fixture.sums(&format!("{digest}  {ASSET}"));
        assert_eq!(
            resolve(&pin(&digest), &fixture.host()).ok(),
            Some(installed)
        );
        assert_eq!(fixture.calls(), ["path --version"]);
        assert!(fixture.store().is_empty(), "{:?}", fixture.store());
    }

    #[test]
    fn another_version_on_path_with_nothing_to_fetch_is_refused_naming_both_versions() {
        let fixture = Fixture::new();
        let installed = fixture.on_path("ess 0.98.0\n");
        let refusal = refusal(&pin(OTHER_DIGEST), &fixture.host());
        assert!(refusal.contains("ess 7.8.9 is needed"), "{refusal}");
        assert!(
            refusal.contains(&format!(
                "{} (PATH): reports \"ess 0.98.0\\n\"",
                installed.display()
            )),
            "{refusal}"
        );
        assert!(
            refusal.contains(&format!(
                "{}: absent",
                fixture.at("store").join(DIRECTORY).join("ess").display()
            )),
            "{refusal}"
        );
        assert!(refusal.contains("fetch: "), "{refusal}");
        assert_eq!(fixture.calls(), ["path --version"]);
        assert!(fixture.store().is_empty(), "{:?}", fixture.store());
    }

    #[test]
    fn an_answer_that_is_not_exactly_the_pinned_version_is_refused() {
        let fixture = Fixture::new();
        for reply in [
            "",
            "\n",
            "7.8.9\n",
            "ess\n",
            "ess 7.8.90\n",
            "ess 7.8.9-rc1\n",
            "ess 7.8.9.1\n",
            "ess 7.8.9 (abc)\n",
            "ess 17.8.9\n",
            "ess 7.8.9 \n",
            "ess 7.8.9\n\n",
            "ess  7.8.9\n",
            " ess 7.8.9\n",
            "ESS 7.8.9\n",
            "ess 7.8\n",
            "ess 7.8.9\ness 0.1.0\n",
            "ess 0.1.0\ness 7.8.9\n",
            "essential 7.8.9\n",
        ] {
            fixture.on_path(reply);
            let refusal = refusal(&pin(OTHER_DIGEST), &fixture.host());
            assert!(
                refusal.contains(&format!("(PATH): reports {reply:?}")),
                "{reply:?}: {refusal}"
            );
        }
        for reply in ["ess 7.8.9\n", "ess 7.8.9"] {
            let installed = fixture.on_path(reply);
            assert_eq!(
                resolve(&pin(OTHER_DIGEST), &fixture.host()).ok(),
                Some(installed),
                "{reply:?}"
            );
        }
    }

    #[test]
    fn an_ess_that_ends_non_zero_is_refused_whatever_it_writes() {
        let fixture = Fixture::new();
        let installed = fixture.at("bin/ess");
        fixture.stand_in(&installed, "path", "ess 7.8.9\n", 3);
        let refusal = refusal(&pin(OTHER_DIGEST), &fixture.host());
        assert!(
            refusal.contains("(PATH): `--version` ended with exit status: 3"),
            "{refusal}"
        );
    }

    #[test]
    fn no_ess_anywhere_is_refused() {
        let fixture = Fixture::new();
        let refusal = refusal(&pin(OTHER_DIGEST), &fixture.host());
        assert!(refusal.contains("ess 7.8.9 is needed"), "{refusal}");
        assert!(refusal.contains(": absent"), "{refusal}");
        assert!(refusal.contains("PATH: no `ess`"), "{refusal}");
        assert!(refusal.contains("fetch: "), "{refusal}");
        assert!(fixture.store().is_empty(), "{:?}", fixture.store());

        let mut host = fixture.host();
        host.path = None;
        let refusal = super::resolve(&pin(OTHER_DIGEST), &host)
            .expect_err("no PATH at all")
            .to_string();
        assert!(refusal.contains("PATH: no `ess`"), "{refusal}");
    }

    #[test]
    fn only_the_first_executable_ess_on_path_counts() {
        let fixture = Fixture::new();
        fixture.on_path("ess 0.98.0\n");
        let second = fixture.at("second/ess");
        fixture.stand_in(&second, "second", "ess 7.8.9\n", 0);
        let mut host = fixture.host();
        host.path = Some(
            std::env::join_paths([fixture.at("bin"), fixture.at("second")]).expect("two entries"),
        );
        let refusal = refusal(&pin(OTHER_DIGEST), &host);
        assert!(refusal.contains("reports \"ess 0.98.0\\n\""), "{refusal}");
        assert_eq!(fixture.calls(), ["path --version"]);

        // A file a shell would not start is passed over, as a shell passes over it.
        fs::set_permissions(fixture.at("bin/ess"), fs::Permissions::from_mode(0o644))
            .expect("the first is no longer executable");
        assert_eq!(resolve(&pin(OTHER_DIGEST), &host).ok(), Some(second));
    }

    #[test]
    fn a_stored_file_that_does_not_start_is_not_used_and_not_replaced() {
        let fixture = Fixture::new();
        let stored = fixture.stored("ess 7.8.9\n");
        fs::set_permissions(&stored, fs::Permissions::from_mode(0o644)).expect("not executable");
        let before = fs::read(&stored).expect("the stored file");
        fixture.on_path("ess 0.98.0\n");
        let digest = fixture.release("ess 7.8.9\n");
        fixture.sums(&format!("{digest}  {ASSET}"));
        let refusal = refusal(&pin(&digest), &fixture.host());
        assert!(
            refusal.contains(&format!("{}: does not start", stored.display())),
            "{refusal}"
        );
        assert!(refusal.contains("remove it"), "{refusal}");
        assert_eq!(fixture.calls(), ["path --version"]);
        assert_eq!(fs::read(&stored).expect("the stored file"), before);

        // The installed one of the pinned version is still taken.
        let installed = fixture.on_path("ess 7.8.9\n");
        assert_eq!(
            resolve(&pin(&digest), &fixture.host()).ok(),
            Some(installed)
        );
    }

    #[test]
    fn a_stored_ess_of_another_version_is_not_used_and_not_replaced() {
        let fixture = Fixture::new();
        let stored = fixture.stored("ess 0.1.0\n");
        let before = fs::read(&stored).expect("the stored file");
        fixture.on_path("ess 0.98.0\n");
        let digest = fixture.release("ess 7.8.9\n");
        fixture.sums(&format!("{digest}  {ASSET}"));
        let refusal = refusal(&pin(&digest), &fixture.host());
        assert!(
            refusal.contains(&format!("{}: reports \"ess 0.1.0\\n\"", stored.display())),
            "{refusal}"
        );
        assert!(refusal.contains("remove it"), "{refusal}");
        assert!(refusal.contains("reports \"ess 0.98.0\\n\""), "{refusal}");
        assert_eq!(fixture.calls(), ["stored --version", "path --version"]);
        assert_eq!(fs::read(&stored).expect("the stored file"), before);
    }

    #[test]
    fn a_missing_ess_is_fetched_checked_stored_and_then_used_from_the_store() {
        let fixture = Fixture::new();
        fixture.on_path("ess 0.98.0\n");
        let digest = fixture.release("ess 7.8.9\n");
        fixture.sums(&format!("{digest}  {ASSET}"));
        let stored = fixture.at("store").join(DIRECTORY).join("ess");
        assert_eq!(
            resolve(&pin(&digest), &fixture.host()).ok(),
            Some(stored.clone())
        );
        assert_eq!(fixture.calls(), ["path --version", "fetched --version"]);
        assert_eq!(fixture.store(), [DIRECTORY], "no download is left behind");

        // The second resolution needs neither the release nor PATH.
        fs::remove_dir_all(fixture.at("release")).expect("the release goes away");
        fs::remove_file(fixture.at("bin/ess")).expect("the installed one goes away");
        assert_eq!(resolve(&pin(&digest), &fixture.host()).ok(), Some(stored));
        assert_eq!(
            fixture.calls(),
            ["path --version", "fetched --version", "fetched --version"]
        );
    }

    #[test]
    fn fetched_bytes_with_another_digest_are_not_started_and_not_kept() {
        let fixture = Fixture::new();
        let digest = fixture.release("ess 7.8.9\n");
        // The release's own list agrees with the pin; only the bytes do not.
        fixture.sums(&format!("{OTHER_DIGEST}  {ASSET}"));
        let refusal = refusal(&pin(OTHER_DIGEST), &fixture.host());
        assert!(
            refusal.contains(&format!(
                "fetch: {ASSET} has SHA-256 {digest}, the pin is {OTHER_DIGEST}"
            )),
            "{refusal}"
        );
        assert!(fixture.calls().is_empty(), "{:?}", fixture.calls());
        assert!(fixture.store().is_empty(), "{:?}", fixture.store());
    }

    #[test]
    fn a_release_whose_line_for_the_asset_is_not_the_pinned_digest_is_not_used() {
        let fixture = Fixture::new();
        let digest = fixture.release("ess 7.8.9\n");
        for line in [
            // Another digest for the asset; the pinned digest only for another file.
            format!("{OTHER_DIGEST}  {ASSET}\n{digest}  other.tar.gz"),
            // The pinned digest on a line that only starts or ends like the asset's.
            format!("{digest}  {ASSET}.sig"),
            format!("{digest}  old-{ASSET}"),
            format!("{digest} {ASSET}"),
            String::new(),
        ] {
            fixture.sums(&line);
            let refusal = refusal(&pin(&digest), &fixture.host());
            assert!(
                refusal.contains(&format!(
                    "fetch: SHA256SUMS of the release has no line `{digest}  {ASSET}`"
                )),
                "{line:?}: {refusal}"
            );
            assert!(fixture.calls().is_empty(), "{:?}", fixture.calls());
            assert!(fixture.store().is_empty(), "{:?}", fixture.store());
        }
    }

    #[test]
    fn a_fetched_ess_that_reports_another_version_is_not_stored() {
        let fixture = Fixture::new();
        let digest = fixture.release("ess 0.1.0\n");
        fixture.sums(&format!("{digest}  {ASSET}"));
        let refusal = refusal(&pin(&digest), &fixture.host());
        assert!(
            refusal.contains(&format!(
                "fetch: the `ess` in {ASSET} reports \"ess 0.1.0\\n\""
            )),
            "{refusal}"
        );
        assert_eq!(fixture.calls(), ["fetched --version"]);
        assert!(fixture.store().is_empty(), "{:?}", fixture.store());
    }

    #[test]
    fn a_release_that_is_not_there_is_a_refusal_and_leaves_nothing() {
        let fixture = Fixture::new();
        let refusal = refusal(&pin(OTHER_DIGEST), &fixture.host());
        assert!(refusal.contains("fetch: curl"), "{refusal}");
        assert!(fixture.store().is_empty(), "{:?}", fixture.store());
    }

    #[test]
    fn where_no_asset_is_fetched_the_refusal_says_so_and_asks_no_release() {
        let fixture = Fixture::new();
        fixture.on_path("ess 0.98.0\n");
        let digest = fixture.release("ess 7.8.9\n");
        fixture.sums(&format!("{digest}  {ASSET}"));
        let mut host = fixture.host();
        host.fetches = false;
        let refusal = refusal(&pin(&digest), &host);
        assert!(
            refusal.contains("fetch: not on this platform; the pinned digest is of the x86_64-unknown-linux-gnu asset"),
            "{refusal}"
        );
        assert!(refusal.contains("reports \"ess 0.98.0\\n\""), "{refusal}");
        assert_eq!(fixture.calls(), ["path --version"]);
        assert!(fixture.store().is_empty(), "{:?}", fixture.store());
    }

    fn workflow(version: &str, digest: &str) -> String {
        format!(
            "jobs:\n  libraries-and-plan:\n    steps:\n      - name: Install cargo-nextest\n        env:\n          NEXTEST_VERSION: \"0.9.143\"\n      - name: Install ess\n        env:\n          ESS_VERSION: {version}\n          ESS_SHA256: {digest}\n        run: echo\n"
        )
    }

    #[test]
    fn the_pin_is_the_version_and_digest_of_the_install_step() {
        assert_eq!(
            Pin::parse(&workflow("\"7.8.9\"", OTHER_DIGEST)),
            Ok(pin(OTHER_DIGEST))
        );
        let other_step =
            workflow("\"7.8.9\"", OTHER_DIGEST).replace("Install ess", "Install another");
        for (text, why) in [
            (other_step.as_str(), "no step \"Install ess\""),
            (&workflow("\"7.8\"", OTHER_DIGEST), "ESS_VERSION"),
            (&workflow("\"7.8.9-rc1\"", OTHER_DIGEST), "ESS_VERSION"),
            (&workflow("\"7.8.9.1\"", OTHER_DIGEST), "ESS_VERSION"),
            (&workflow("\"7.8.x\"", OTHER_DIGEST), "ESS_VERSION"),
            (&workflow("\"7.8.+9\"", OTHER_DIGEST), "ESS_VERSION"),
            (&workflow("\"../7.8.9\"", OTHER_DIGEST), "ESS_VERSION"),
            (&workflow("7.8", OTHER_DIGEST), "ESS_VERSION"),
            (&workflow("\"7.8.9\"", "00aa"), "ESS_SHA256"),
            (
                &workflow("\"7.8.9\"", &OTHER_DIGEST.to_uppercase()),
                "ESS_SHA256",
            ),
            (
                &workflow("\"7.8.9\"", &OTHER_DIGEST.replace('a', "g")),
                "ESS_SHA256",
            ),
            ("jobs: [", "YAML"),
        ] {
            let refused = Pin::parse(text).expect_err(why);
            assert!(refused.contains(why), "{why}: {refused}");
        }
    }

    #[test]
    fn the_repositorys_workflow_holds_a_pin() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pin = Pin::read(&root).expect("gate.yml pins ess");
        assert_eq!(pin.version.split('.').count(), 3, "{pin:?}");
        assert_eq!(pin.sha256.len(), 64, "{pin:?}");
        let missing = Pin::read(&root.join("checks")).expect_err("no workflow there");
        assert!(missing.contains(".github/workflows/gate.yml"), "{missing}");
    }

    #[test]
    fn the_store_is_under_target_and_the_releases_are_githubs_unless_a_variable_says_otherwise() {
        let root = Path::new("/repository");
        let host = Host::of(root, |name| {
            (name == "PATH").then(|| OsString::from("/usr/bin"))
        });
        assert_eq!(host.store, Path::new("/repository/target/ess"));
        assert_eq!(host.releases, RELEASES);
        assert_eq!(host.path, Some(OsString::from("/usr/bin")));

        let host = Host::of(root, |name| match name {
            STORE_VARIABLE => Some(OsString::from("/elsewhere")),
            RELEASES_VARIABLE => Some(OsString::from("file:///releases")),
            _ => None,
        });
        assert_eq!(host.store, Path::new("/elsewhere"));
        assert_eq!(host.releases, "file:///releases");
        assert_eq!(host.path, None);

        let host = Host::of(root, |_| Some(OsString::new()));
        assert_eq!(host.store, Path::new("/repository/target/ess"));
        assert_eq!(host.releases, RELEASES);
    }

    /// Publishes a release whose `ess` notes its call, runs `first` and then writes the pinned
    /// version, and returns the asset's digest.
    fn adversary_w32_release_that_first_runs(fixture: &Fixture, first: &str) -> String {
        fixture.release("ess 7.8.9\n");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"fetched $*\" >> '{}'\n{first}\nprintf '%s\\n' 'ess 7.8.9'\n",
            fixture.at("calls").display()
        );
        let member = fixture.at("tree").join(DIRECTORY).join("ess");
        fs::write(&member, script).expect("the member is written");
        fixture.release_again()
    }

    impl Fixture {
        /// Packs `tree/` again as the release's asset and returns its digest.
        fn release_again(&self) -> String {
            let release = self.at("release").join(VERSION);
            let packed = Command::new("tar")
                .arg("-czf")
                .arg(release.join(ASSET))
                .arg("-C")
                .arg(self.at("tree"))
                .arg(DIRECTORY)
                .status()
                .expect("tar runs");
            assert!(packed.success(), "tar packs the asset");
            let mut digest = String::new();
            for byte in Sha256::digest(fs::read(release.join(ASSET)).expect("the asset")) {
                write!(digest, "{byte:02x}").expect("a string takes text");
            }
            digest
        }
    }

    /// Two runs of the gate fetch at once and the other one stores the release first: this run
    /// uses the stored one and leaves no download behind. The other run is played by the fetched
    /// `ess` itself, which stores a copy of itself when it is asked for its version.
    #[test]
    fn adversary_w32_a_release_another_run_stored_during_the_fetch_is_used() {
        let fixture = Fixture::new();
        let place = fixture.at("store").join(DIRECTORY);
        let stored = place.join("ess");
        let digest = adversary_w32_release_that_first_runs(
            &fixture,
            &format!(
                "[ -e '{0}' ] || {{ mkdir -p '{1}' && cp \"$0\" '{0}'; }}",
                stored.display(),
                place.display()
            ),
        );
        fixture.sums(&format!("{digest}  {ASSET}"));
        assert_eq!(
            resolve(&pin(&digest), &fixture.host()).ok(),
            Some(stored.clone())
        );
        assert_eq!(
            fixture.calls(),
            ["fetched --version", "fetched --version"],
            "the unpacked one, then the one the other run stored"
        );
        assert_eq!(fixture.store(), [DIRECTORY], "no download is left behind");
        assert_eq!(
            fs::read_dir(&place).expect("the stored release").count(),
            1,
            "the stored release is the other run's, whole"
        );
    }

    /// A directory in the stored release's place that holds no `ess` is not a stored release:
    /// the fetch cannot move its own there, so the resolution is a refusal that names the place,
    /// the directory keeps what it held and no download is left behind.
    #[test]
    fn adversary_w32_a_directory_without_an_ess_in_the_stored_place_is_a_refusal() {
        let fixture = Fixture::new();
        let place = fixture.at("store").join(DIRECTORY);
        fs::create_dir_all(&place).expect("the place");
        fs::write(place.join("kept"), "not an ess").expect("a file that is not an ess");
        let digest = fixture.release("ess 7.8.9\n");
        fixture.sums(&format!("{digest}  {ASSET}"));
        let refusal = refusal(&pin(&digest), &fixture.host());
        assert!(
            refusal.contains(&format!("fetch: {}: ", place.display())),
            "{refusal}"
        );
        assert_eq!(fixture.store(), [DIRECTORY], "no download is left behind");
        assert_eq!(
            fs::read(place.join("kept")).expect("the file is still there"),
            b"not an ess"
        );
        assert!(!place.join("ess").exists(), "nothing was moved into it");
        assert!(refusal.contains("remove it"), "{refusal}");
        assert!(
            fixture.calls().is_empty(),
            "nothing was fetched, so no fetched `ess` was started: {:?}",
            fixture.calls()
        );
    }

    /// What a shell does not start is passed over wherever it comes before an `ess` it does
    /// start: a directory named `ess` and a link named `ess` that leads nowhere.
    #[test]
    fn adversary_w32_a_directory_and_a_dangling_link_named_ess_are_passed_over() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.at("bin/ess")).expect("a directory named ess");
        fs::create_dir_all(fixture.at("links")).expect("a second PATH directory");
        std::os::unix::fs::symlink(fixture.at("nowhere"), fixture.at("links/ess"))
            .expect("a link that leads nowhere");
        let third = fixture.at("third/ess");
        fixture.stand_in(&third, "third", "ess 7.8.9\n", 0);
        let mut host = fixture.host();
        host.path = Some(
            std::env::join_paths([fixture.at("bin"), fixture.at("links"), fixture.at("third")])
                .expect("three entries"),
        );
        assert_eq!(resolve(&pin(OTHER_DIGEST), &host).ok(), Some(third));
        assert_eq!(fixture.calls(), ["third --version"]);
    }

    /// An `ess` on `PATH` that is a link to an executable is taken, as a shell takes it.
    #[test]
    fn adversary_w32_a_link_named_ess_to_an_executable_is_taken() {
        let fixture = Fixture::new();
        let linked = fixture.at("elsewhere/ess-7.8.9");
        fixture.stand_in(&linked, "linked", "ess 7.8.9\n", 0);
        std::os::unix::fs::symlink(&linked, fixture.at("bin/ess")).expect("a link named ess");
        assert_eq!(
            resolve(&pin(OTHER_DIGEST), &fixture.host()).ok(),
            Some(fixture.at("bin/ess"))
        );
        assert_eq!(fixture.calls(), ["linked --version"]);
    }

    /// Only the `ess` of the asset is stored: whatever else the archive holds beside it stays
    /// in the download that is removed.
    #[test]
    fn adversary_w32_only_the_ess_of_the_asset_is_stored() {
        let fixture = Fixture::new();
        fixture.release("ess 7.8.9\n");
        fs::write(fixture.at("tree").join(DIRECTORY).join("beside"), "x").expect("beside");
        let digest = fixture.release_again();
        fixture.sums(&format!("{digest}  {ASSET}"));
        let stored = fixture.at("store").join(DIRECTORY).join("ess");
        assert_eq!(
            resolve(&pin(&digest), &fixture.host()).ok(),
            Some(stored.clone())
        );
        let held: Vec<PathBuf> = fs::read_dir(fixture.at("store").join(DIRECTORY))
            .expect("the stored release")
            .map(|entry| entry.expect("an entry").path())
            .collect();
        assert_eq!(held, [stored]);
    }

    /// Whatever is in the stored release's place without an `ess` in it is refused before any
    /// download, an empty directory and a file as well: the release is not asked for, so the
    /// refusal is the same with none published, and with one published nothing of it is started.
    #[test]
    fn whatever_is_in_the_stored_place_without_an_ess_is_refused_before_any_download() {
        for shape in ["an empty directory", "a file"] {
            let fixture = Fixture::new();
            let place = fixture.at("store").join(DIRECTORY);
            fs::create_dir_all(fixture.at("store")).expect("the store");
            if shape == "a file" {
                fs::write(&place, "not a release").expect("a file in the place");
            } else {
                fs::create_dir(&place).expect("an empty directory in the place");
            }
            let words = format!(
                "fetch: {}: is there and holds no `ess`; remove it and the release is fetched again",
                place.display()
            );

            // No release is published: a download would be a refusal that names `curl`.
            let unpublished = refusal(&pin(OTHER_DIGEST), &fixture.host());
            assert!(unpublished.contains(&words), "{shape}: {unpublished}");
            assert!(!unpublished.contains("curl"), "{shape}: {unpublished}");

            let digest = fixture.release("ess 7.8.9\n");
            fixture.sums(&format!("{digest}  {ASSET}"));
            let published = refusal(&pin(&digest), &fixture.host());
            assert!(published.contains(&words), "{shape}: {published}");
            assert!(fixture.calls().is_empty(), "{shape}: {:?}", fixture.calls());
            assert_eq!(fixture.store(), [DIRECTORY], "{shape}: nothing was added");
            assert!(
                !place.join("ess").exists(),
                "{shape}: nothing was moved there"
            );
        }
    }

    /// Another run of the gate stores the release after this one found the stored place empty
    /// and before it comes to fetch: what is stored then is looked at again, used when it reports
    /// the pinned version and named when it does not, and nothing is downloaded either way. The
    /// other run is played by the `ess` on `PATH`, which moves a release to the stored place
    /// when it is asked for its version.
    #[test]
    fn a_release_another_run_stored_while_path_was_asked_is_looked_at_again() {
        for (reply, used) in [("ess 7.8.9\n", true), ("ess 7.8.8\n", false)] {
            let fixture = Fixture::new();
            let ready = fixture.at("ready").join(DIRECTORY);
            fixture.stand_in(&ready.join("ess"), "stored", reply, 0);
            let place = fixture.at("store").join(DIRECTORY);
            let script = format!(
                "#!/bin/sh\nprintf '%s\\n' \"path $*\" >> '{}'\nmkdir -p '{}' && mv '{}' '{}'\nprintf '%s\\n' 'ess 0.98.0'\n",
                fixture.at("calls").display(),
                fixture.at("store").display(),
                ready.display(),
                place.display()
            );
            fs::write(fixture.at("bin/ess"), script).expect("the `ess` on PATH is written");
            fs::set_permissions(fixture.at("bin/ess"), fs::Permissions::from_mode(0o755))
                .expect("the `ess` on PATH is executable");

            // No release is published: a download would be a refusal that names `curl`.
            let found = resolve(&pin(OTHER_DIGEST), &fixture.host());
            if used {
                assert_eq!(
                    found.map_err(|refusal| refusal.to_string()),
                    Ok(place.join("ess"))
                );
            } else {
                let said = found.expect_err("another version is not used").to_string();
                assert!(
                    said.contains(&format!(
                        "{}: reports \"ess 7.8.8\\n\"; remove it and the release is fetched again",
                        place.join("ess").display()
                    )),
                    "{said}"
                );
                assert!(!said.contains("holds no `ess`"), "{said}");
                assert!(!said.contains("curl"), "{said}");
            }
            assert_eq!(fixture.calls(), ["path --version", "stored --version"]);
            assert_eq!(fixture.store(), [DIRECTORY], "nothing was downloaded");
        }
    }

    /// What a run that a signal ended left in the store is never read. With a `.fetch-*`
    /// directory that holds an `ess` wherever a release has one, the next run fetches the
    /// release, stores it and uses it, starts nothing of the leftover and leaves it where it is.
    #[test]
    fn a_leftover_of_an_interrupted_fetch_is_never_read_and_the_release_is_fetched() {
        let fixture = Fixture::new();
        let leftover = fixture.at("store").join(".fetch-left");
        fixture.stand_in(&leftover.join("ess"), "leftover", "ess 7.8.9\n", 0);
        fixture.stand_in(
            &leftover.join(DIRECTORY).join("ess"),
            "leftover",
            "ess 7.8.9\n",
            0,
        );
        fs::write(leftover.join(ASSET), "half a download").expect("a cut download");
        fixture.on_path("ess 0.98.0\n");
        let digest = fixture.release("ess 7.8.9\n");
        fixture.sums(&format!("{digest}  {ASSET}"));
        let stored = fixture.at("store").join(DIRECTORY).join("ess");
        assert_eq!(
            resolve(&pin(&digest), &fixture.host()).ok(),
            Some(stored.clone())
        );
        assert_eq!(fixture.calls(), ["path --version", "fetched --version"]);
        let mut held = fixture.store();
        held.sort();
        assert_eq!(
            held,
            [".fetch-left", DIRECTORY],
            "the leftover stays and the release is stored beside it"
        );
        assert_eq!(
            fs::read(leftover.join(ASSET)).expect("the cut download"),
            b"half a download"
        );

        // From then on the stored release is used, and the leftover still is not.
        assert_eq!(resolve(&pin(&digest), &fixture.host()).ok(), Some(stored));
        assert_eq!(
            fixture.calls(),
            ["path --version", "fetched --version", "fetched --version"]
        );
    }
}

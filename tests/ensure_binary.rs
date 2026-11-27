//! `scripts/ensure-binary.sh`, the plugin's SessionStart installer, run for
//! real with a stand-in `curl` first on the `PATH` and a temporary `HOME`.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/ensure-binary.sh");
const LATEST_TAG: &str = "v2.0.0";
const NEW_BINARY: &str = "#!/bin/sh\necho new cartographer\n";
const OLD_BINARY: &str = "#!/bin/sh\necho old cartographer\n";

/// Answers the release lookup with `FAKE_TAG`. For the asset it behaves as
/// curl does when the server answers 404 and `FAKE_DOWNLOAD` is `missing`:
/// with `-f` it fails with status 22 and writes nothing; without it, it
/// saves the error page and succeeds.
const FAKE_CURL: &str = r#"#!/bin/sh
out=""
url=""
fail_on_http_error=no
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out="$2"; shift ;;
    --fail) fail_on_http_error=yes ;;
    --*) ;;
    -*f*) fail_on_http_error=yes ;;
    -*) ;;
    *) url="$1" ;;
  esac
  shift
done
case "$url" in
  *api.github.com*)
    printf '{\n  "tag_name": "%s"\n}\n' "$FAKE_TAG"
    ;;
  *)
    if [ "$FAKE_DOWNLOAD" = "missing" ]; then
      if [ "$fail_on_http_error" = yes ]; then
        echo "curl: (22) The requested URL returned error: 404" >&2
        exit 22
      fi
      printf 'Not Found' > "$out"
    else
      printf '#!/bin/sh\necho new cartographer\n' > "$out"
    fi
    ;;
esac
"#;

struct Machine {
    home: TempDir,
    fake_bin: TempDir,
}

impl Machine {
    fn new() -> Self {
        let fake_bin = tempfile::tempdir().unwrap();
        let curl = fake_bin.path().join("curl");
        fs::write(&curl, FAKE_CURL).unwrap();
        fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            home: tempfile::tempdir().unwrap(),
            fake_bin,
        }
    }

    fn bin_dir(&self) -> PathBuf {
        self.home.path().join(".cartographer/bin")
    }

    fn binary(&self) -> PathBuf {
        self.bin_dir().join("cartographer")
    }

    fn version_file(&self) -> PathBuf {
        self.home.path().join(".cartographer/.version")
    }

    /// Puts an earlier install in place.
    fn with_installed(self, tag: &str) -> Self {
        fs::create_dir_all(self.bin_dir()).unwrap();
        fs::write(self.binary(), OLD_BINARY).unwrap();
        fs::set_permissions(self.binary(), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(self.version_file(), format!("{tag}\n")).unwrap();
        self
    }

    fn run(&self, download: &str) -> Output {
        let path = format!(
            "{}:{}",
            self.fake_bin.path().display(),
            std::env::var("PATH").unwrap()
        );
        Command::new("sh")
            .arg(SCRIPT)
            .env("HOME", self.home.path())
            .env("PATH", path)
            .env("FAKE_TAG", LATEST_TAG)
            .env("FAKE_DOWNLOAD", download)
            .output()
            .unwrap()
    }

    fn files_in_bin_dir(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.bin_dir())
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).unwrap().permissions().mode() & 0o111 != 0
}

#[test]
fn a_good_download_installs_the_binary_and_records_the_version() {
    let machine = Machine::new();

    let output = machine.run("ok");

    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(fs::read_to_string(machine.binary()).unwrap(), NEW_BINARY);
    assert!(is_executable(&machine.binary()));
    assert_eq!(
        fs::read_to_string(machine.version_file()).unwrap().trim(),
        LATEST_TAG
    );
    assert_eq!(machine.files_in_bin_dir(), vec!["cartographer"]);
    assert!(said(&output).contains("installed successfully"));
}

#[test]
fn a_failed_download_keeps_the_previous_binary_and_says_so() {
    let machine = Machine::new().with_installed("v1.0.0");

    let output = machine.run("missing");

    assert!(!output.status.success(), "{}", said(&output));
    assert_eq!(fs::read_to_string(machine.binary()).unwrap(), OLD_BINARY);
    assert_eq!(
        fs::read_to_string(machine.version_file()).unwrap().trim(),
        "v1.0.0",
        "the version must not move to a release that was not installed"
    );
    assert_eq!(machine.files_in_bin_dir(), vec!["cartographer"]);
    let said = said(&output);
    assert!(!said.contains("installed successfully"), "{said}");
    assert!(
        said.contains("download") && said.contains(LATEST_TAG),
        "{said}"
    );
}

#[test]
fn a_failed_download_on_a_fresh_machine_installs_nothing() {
    let machine = Machine::new();

    let output = machine.run("missing");

    assert!(!output.status.success(), "{}", said(&output));
    assert!(!machine.binary().exists());
    assert!(!machine.version_file().exists());
    assert_eq!(machine.files_in_bin_dir(), Vec::<String>::new());
    assert!(!said(&output).contains("installed successfully"));
}

#[test]
fn the_latest_version_already_installed_is_left_alone() {
    let machine = Machine::new().with_installed(LATEST_TAG);

    let output = machine.run("missing");

    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(fs::read_to_string(machine.binary()).unwrap(), OLD_BINARY);
}

// ============================================================================
// scripts/run-server.sh, the command the plugin registers as its MCP server
// ============================================================================

const RUN_SERVER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/run-server.sh");

impl Machine {
    fn run_server(&self, download: &str, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.fake_bin.path().display(),
            std::env::var("PATH").unwrap()
        );
        Command::new("sh")
            .arg(RUN_SERVER)
            .args(args)
            .env("HOME", self.home.path())
            .env("PATH", path)
            .env("FAKE_TAG", LATEST_TAG)
            .env("FAKE_DOWNLOAD", download)
            .output()
            .unwrap()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_server_command_runs_the_installed_binary_with_its_arguments() {
    let machine = Machine::new().with_installed("v1.0.0");
    fs::write(machine.binary(), "#!/bin/sh\necho \"ran with: $*\"\n").unwrap();

    // The download would fail: an installed binary must not need the network.
    let output = machine.run_server("missing", &["--parse-only"]);

    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(stdout(&output), "ran with: --parse-only\n");
}

// On the first session after install the server may start before the
// SessionStart hook has downloaded the binary, and a server that fails its
// first connection is not started again.
#[test]
fn the_server_command_downloads_a_missing_binary_and_then_runs_it() {
    let machine = Machine::new();

    let output = machine.run_server("ok", &[]);

    assert!(output.status.success(), "{}", said(&output));
    // Standard output is the MCP channel: only the binary may write to it.
    assert_eq!(stdout(&output), "new cartographer\n");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Installing"),
        "the installer's progress belongs on standard error: {}",
        said(&output)
    );
    assert_eq!(
        fs::read_to_string(machine.version_file()).unwrap().trim(),
        LATEST_TAG
    );
}

#[test]
fn the_server_command_fails_plainly_when_there_is_no_binary_to_run() {
    let machine = Machine::new();

    let output = machine.run_server("missing", &[]);

    assert!(!output.status.success(), "{}", said(&output));
    assert_eq!(stdout(&output), "");
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("Failed to download"), "{errors}");
    assert!(errors.contains("not installed"), "{errors}");
}

//! Console-free process spawning and liveness checks.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::{PlatformError, PlatformResult};

/// How to start a child process.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// Executable to run.
    pub program: PathBuf,
    /// Arguments, passed without shell interpretation.
    pub args: Vec<String>,
    /// Working directory.
    pub working_dir: Option<PathBuf>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
}

impl SpawnOptions {
    /// Builds options for `program` with no arguments.
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            working_dir: None,
            env: Vec::new(),
        }
    }

    /// Appends an argument.
    #[must_use]
    pub fn arg(mut self, value: impl Into<String>) -> Self {
        self.args.push(value.into());
        self
    }

    /// Appends several arguments.
    #[must_use]
    pub fn args<I: IntoIterator<Item = S>, S: Into<String>>(mut self, values: I) -> Self {
        self.args.extend(values.into_iter().map(Into::into));
        self
    }

    /// Sets an environment variable for the child.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Sets the working directory.
    #[must_use]
    pub fn working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }
}

/// A spawned child process.
#[derive(Debug)]
pub struct ChildProcess {
    child: Child,
    pid: u32,
}

impl ChildProcess {
    /// The child's process id.
    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.pid
    }

    /// Returns the exit status if the child has already exited.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::Io`] when the status cannot be read.
    pub fn try_wait(&mut self) -> PlatformResult<Option<std::process::ExitStatus>> {
        self.child
            .try_wait()
            .map_err(|e| PlatformError::io("check the browser process", e))
    }

    /// Asks the operating system to terminate the child.
    ///
    /// This kills only the process itself. The surrounding
    /// [`crate::ProcessGroup`] is what cleans up the rest of the tree.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::Io`] when the request fails.
    pub fn kill(&mut self) -> PlatformResult<()> {
        self.child
            .kill()
            .map_err(|e| PlatformError::io("stop the browser process", e))
    }
}

/// Starts a process with no console window and no inherited standard streams.
///
/// A GUI application must never flash a console window, and a browser inheriting
/// this process's pipes would keep them open long after the manager has moved
/// on.
///
/// # Errors
///
/// Returns [`PlatformError::Io`] when the process cannot be started.
pub fn spawn(options: &SpawnOptions) -> PlatformResult<ChildProcess> {
    let mut command = Command::new(&options.program);
    command.args(&options.args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = &options.working_dir {
        command.current_dir(dir);
    }
    for (key, value) in &options.env {
        command.env(key, value);
    }
    apply_no_window(&mut command);
    let child = command
        .spawn()
        .map_err(|e| PlatformError::io("start the browser", e))?;
    let pid = child.id();
    Ok(ChildProcess { child, pid })
}

#[cfg(windows)]
fn apply_no_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW. Declared locally rather than pulling in another windows
    // feature for one constant; the value is fixed by the Win32 ABI.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn apply_no_window(_command: &mut Command) {
    // Console windows are a Windows concept; nothing to suppress.
}

/// Whether a process with this id is currently running.
///
/// Used at startup to decide whether a runtime record left behind by a crash
/// describes a process that is still alive.
#[cfg(windows)]
#[must_use]
pub fn process_is_running(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: `OpenProcess` takes plain values. On success the returned handle is
    // owned by this process and is closed exactly once below. A failure means
    // the process does not exist or is not accessible; both are reported as "not
    // running", which is the safe answer for the caller (it will simply relaunch
    // rather than adopt a process it cannot supervise).
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return false;
    };

    let mut exit_code: u32 = 0;
    // SAFETY: `handle` is valid for the duration of the call, and `exit_code` is
    // a live stack local the callee writes exactly one u32 into.
    let queried = unsafe { GetExitCodeProcess(handle, &raw mut exit_code) };

    // SAFETY: `handle` came from a successful `OpenProcess`, has not been closed,
    // and is closed exactly once here on every path.
    let _ = unsafe { CloseHandle(handle) };

    // STILL_ACTIVE is 259. A process that genuinely exits with 259 is
    // indistinguishable from a running one through this API; the consequence is
    // that a stopped profile is briefly reported as running until the next
    // reconciliation, which is preferable to killing a live browser.
    queried.is_ok() && exit_code == STILL_ACTIVE.0.cast_unsigned()
}

/// Whether a process with this id is currently running.
#[cfg(not(windows))]
#[must_use]
pub fn process_is_running(pid: u32) -> bool {
    // `/proc` is present on the Linux hosts used for development and CI of this
    // crate's platform-independent parts. A process that has exited but not yet
    // been reaped still has a `/proc` entry, so the state field is checked too:
    // a zombie is not something the manager can supervise or focus.
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // The state is the field after the parenthesised command name, which may
    // itself contain spaces and parentheses.
    let Some(after_name) = stat.rsplit_once(") ") else {
        return false;
    };
    !after_name.1.starts_with('Z')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_program() -> SpawnOptions {
        if cfg!(windows) {
            SpawnOptions::new("cmd.exe").args(["/C", "exit 0"])
        } else {
            SpawnOptions::new("/bin/sh").args(["-c", "exit 0"])
        }
    }

    fn sleep_program(seconds: u32) -> SpawnOptions {
        if cfg!(windows) {
            SpawnOptions::new("cmd.exe").args(["/C", &format!("timeout /T {seconds} /NOBREAK")])
        } else {
            SpawnOptions::new("/bin/sh").args(["-c", &format!("sleep {seconds}")])
        }
    }

    #[test]
    fn options_build_up_a_command_line() {
        let options = SpawnOptions::new("thorium.exe")
            .arg("--user-data-dir=C:\\p\\a")
            .args(["--lang=pl-PL", "https://example.test/"])
            .env("TZ", "Europe/Warsaw")
            .working_dir("C:\\p");
        assert_eq!(options.args.len(), 3);
        assert_eq!(options.args[0], "--user-data-dir=C:\\p\\a");
        assert_eq!(options.env, vec![("TZ".to_owned(), "Europe/Warsaw".to_owned())]);
        assert_eq!(
            options.working_dir.as_deref(),
            Some(std::path::Path::new("C:\\p"))
        );
    }

    #[test]
    fn a_spawned_process_reports_a_pid_and_exits() {
        let mut child = spawn(&echo_program()).expect("spawn");
        assert!(child.pid() > 0);
        // Wait briefly for it to finish; the exact timing is not what is being
        // tested, only that the status becomes observable.
        for _ in 0..200 {
            if child.try_wait().expect("try_wait").is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("the child process did not exit");
    }

    #[test]
    fn a_running_process_is_detected_and_a_finished_one_is_not() {
        let mut child = spawn(&sleep_program(30)).expect("spawn");
        let pid = child.pid();
        assert!(
            process_is_running(pid),
            "a just-spawned process must be reported as running"
        );
        child.kill().expect("kill");
        for _ in 0..200 {
            // Reaping the child is part of the wait: an unreaped process still
            // occupies its id.
            let _ = child.try_wait();
            if !process_is_running(pid) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("a killed process is still reported as running");
    }

    #[test]
    fn a_missing_executable_is_reported_not_panicked() {
        let err = spawn(&SpawnOptions::new("definitely-not-a-real-program-xyzzy")).expect_err("must fail");
        assert!(matches!(err, PlatformError::Io { .. }));
    }

    #[test]
    fn an_implausible_pid_is_not_running() {
        // A pid that cannot correspond to a live process on either platform.
        assert!(!process_is_running(u32::MAX - 1));
    }

    #[test]
    fn a_spawned_process_can_be_placed_in_a_process_group() {
        let mut child = spawn(&sleep_program(30)).expect("spawn");
        let group = crate::ProcessGroup::new().expect("job");
        group.assign(child.pid()).expect("assign");
        group.terminate().expect("terminate");
        let _ = child.kill();
        let _ = child.try_wait();
    }
}

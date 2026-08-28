//! Windows process-tree cleanup.
//!
//! Chromium is a process tree: killing only the process the manager launched
//! leaves renderers, the GPU process and the network service behind. These tests
//! prove the Job Object actually removes the whole tree, including grandchildren
//! the manager never saw.
//!
//! They run only on Windows, where Job Objects exist. The Unix-side guarantees
//! this crate makes are covered by `isolation.rs`.

#![cfg(windows)]

use std::time::Duration;

use tw_windows_platform::{ProcessGroup, SpawnOptions, process_is_running, spawn};

/// Waits for `predicate` to hold, up to roughly ten seconds.
fn wait_until(mut predicate: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Starts a process that outlives the test unless something kills it, and
/// records its own child's process id into `marker`.
///
/// `cmd /C start /B` makes the inner `timeout` a grandchild: it is the shape
/// that matters, because a grandchild is exactly what a naive kill misses.
fn spawn_tree(marker: &std::path::Path) -> tw_windows_platform::ChildProcess {
    let script = format!(
        "for /f \"tokens=2\" %a in ('tasklist /FI \"IMAGENAME eq timeout.exe\" /NH') do @echo %a > \"{}\" & timeout /T 300 /NOBREAK",
        marker.display()
    );
    spawn(&SpawnOptions::new("cmd.exe").args(["/C", &script])).expect("spawn a process tree")
}

#[test]
fn dropping_the_job_terminates_the_whole_process_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join("child.txt");

    let pid = {
        let group = ProcessGroup::new().expect("job");
        let child = spawn_tree(&marker);
        let pid = child.pid();
        group.assign(pid).expect("assign");
        assert!(wait_until(|| process_is_running(pid)), "the tree did not start");
        pid
        // `group` and `child` both drop here. Closing the last job handle is
        // what terminates every process still inside it.
    };

    assert!(
        wait_until(|| !process_is_running(pid)),
        "closing the job must terminate the process it contained"
    );
}

#[test]
fn terminating_the_job_stops_a_running_tree_without_dropping_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join("child.txt");

    let group = ProcessGroup::new().expect("job");
    let mut child = spawn_tree(&marker);
    let pid = child.pid();
    group.assign(pid).expect("assign");
    assert!(wait_until(|| process_is_running(pid)), "the tree did not start");

    group.terminate().expect("terminate");
    assert!(
        wait_until(|| !process_is_running(pid)),
        "TerminateJobObject must stop the tree"
    );

    // The handle is still usable afterwards, and terminating an empty job is
    // harmless.
    group
        .terminate()
        .expect("terminating an empty job is not an error");
    let _ = child.try_wait();
}

#[test]
fn a_process_outside_the_job_is_unaffected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let inside_marker = dir.path().join("inside.txt");
    let outside_marker = dir.path().join("outside.txt");

    let mut outside = spawn_tree(&outside_marker);
    let outside_pid = outside.pid();
    assert!(
        wait_until(|| process_is_running(outside_pid)),
        "the control process did not start"
    );

    {
        let group = ProcessGroup::new().expect("job");
        let inside = spawn_tree(&inside_marker);
        group.assign(inside.pid()).expect("assign");
        assert!(
            wait_until(|| process_is_running(inside.pid())),
            "the supervised process did not start"
        );
    }

    // The job only ever contained what was assigned to it.
    assert!(
        process_is_running(outside_pid),
        "an unrelated process must not be terminated"
    );
    let _ = outside.kill();
    let _ = outside.try_wait();
}

#[test]
fn supervision_is_actually_active_on_windows() {
    assert!(
        ProcessGroup::is_supervising(),
        "Job Object supervision must be active on Windows"
    );
}

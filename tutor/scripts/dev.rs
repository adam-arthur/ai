---
[package]
edition = "2024"

[dependencies]
ctrlc = "3.5.1"

[target.'cfg(unix)'.dependencies]
nix = { version = "0.30.1", features = ["signal"] }
---

use std::{error::Error, io, process::{Child, Command, ExitStatus, Stdio}, sync::{Arc, atomic::{AtomicBool, Ordering}}, thread, time::{Duration, Instant}};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> Result<(), Box<dyn Error>> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let handler_interrupted = Arc::clone(&interrupted);
    ctrlc::set_handler(move || handler_interrupted.store(true, Ordering::SeqCst))?;

    let mut server = spawn("server", "cargo", &["run", "-p", "tutor-server"])?;
    let mut web = match spawn("web", "npm", &["--workspace", "@ai/tutor", "run", "dev"]) {
        Ok(web) => web,
        Err(error) => {
            stop(&mut server);
            return Err(error.into());
        },
    };

    loop {
        if interrupted.load(Ordering::SeqCst) {
            eprintln!("\nShutting down tutor...");
            shutdown(&mut server, &mut web);
            return Ok(());
        }

        let server_status = server.try_wait()?;
        let web_status = web.try_wait()?;
        if server_status.is_some() || web_status.is_some() {
            shutdown(&mut server, &mut web);
            return exited_early(server_status, web_status);
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn spawn(name: &str, program: &str, args: &[&str]) -> io::Result<Child> {
    eprintln!("Starting {name}...");
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    // A separate process group lets us stop descendants such as the server
    // launched by `cargo run` and Vite launched by npm.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }

    command.spawn()
}

fn shutdown(server: &mut Child, web: &mut Child) {
    interrupt(server);
    interrupt(web);

    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    while Instant::now() < deadline {
        let server_done = matches!(server.try_wait(), Ok(Some(_)));
        let web_done = matches!(web.try_wait(), Ok(Some(_)));
        if server_done && web_done {
            return;
        }
        thread::sleep(POLL_INTERVAL);
    }

    stop(server);
    stop(web);
}

#[cfg(unix)]
fn interrupt(child: &mut Child) {
    use nix::{errno::Errno, sys::signal::{Signal, kill}, unistd::Pid};

    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }

    let process_group = Pid::from_raw(-(child.id() as i32));
    if let Err(error) = kill(process_group, Signal::SIGINT)
        && error != Errno::ESRCH
    {
        eprintln!("Failed to interrupt process group: {error}");
    }
}

#[cfg(not(unix))]
fn interrupt(child: &mut Child) {
    stop(child);
}

fn stop(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }

    if let Err(error) = child.kill()
        && error.kind() != io::ErrorKind::InvalidInput
    {
        eprintln!("Failed to stop process: {error}");
    }
    let _ = child.wait();
}

fn exited_early(server_status: Option<ExitStatus>, web_status: Option<ExitStatus>) -> Result<(), Box<dyn Error>> {
    let message = match (server_status, web_status) {
        (Some(server), Some(web)) => format!("server exited with {server}; web exited with {web}"),
        (Some(status), None) => format!("server exited with {status}"),
        (None, Some(status)) => format!("web exited with {status}"),
        (None, None) => unreachable!("called only after a child exits"),
    };
    Err(message.into())
}

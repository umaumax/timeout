use std::io;
use std::process::ExitStatus;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use libc::pid_t;
use structopt::StructOpt;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

pub trait ChildExt {
    fn send_signal(&mut self, signal: i32) -> io::Result<()>;
    fn wait_or_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>>;
}

impl ChildExt for Child {
    fn send_signal(&mut self, signal: i32) -> io::Result<()> {
        if unsafe { libc::kill(self.id() as pid_t, signal as i32) } != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn wait_or_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        if timeout == Duration::from_micros(0) {
            return self.wait().map(|status| Some(status));
        }
        // manually drop stdin becase .try_wait() doesn't
        drop(self.stdin.take());

        let sleep_unit = std::cmp::min(timeout.mul_f64(0.05), Duration::from_millis(100));
        let start = Instant::now();
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(Some(status));
            }

            if start.elapsed() >= timeout {
                break;
            }

            thread::sleep(sleep_unit);
        }

        Ok(None)
    }
}

#[derive(StructOpt)]
struct Cli {
    #[structopt(name = "duration", help = "unshare bandwidth limit flag")]
    duration: String,

    #[structopt(name = "commands")]
    pub commands: Vec<String>,
}

fn main() -> std::io::Result<()> {
    let args = Cli::from_args();

    let duration: Duration = {
        let text = args.duration;
        match text.parse::<f64>() {
            Ok(v) => Duration::from_secs_f64(v),
            _ => match text.parse::<humantime::Duration>() {
                Ok(v) => v.into(),
                _ => parse_duration::parse(&text).unwrap(),
            },
        }
    };
    let command = &args.commands[0];
    let args = &args.commands[1..];

    let mut child = Command::new(command).args(args).spawn().unwrap();

    match child.wait_or_timeout(duration).unwrap() {
        Some(status) => {
            if let Some(code) = status.code() {
                std::process::exit(code);
            } else {
                if cfg!(feature = "unix") {
                    std::process::exit(status.signal().unwrap());
                }
                std::process::exit(1);
            }
        }
        None => {
            child.send_signal(libc::SIGKILL)?;
        }
    };
    Ok(())
}

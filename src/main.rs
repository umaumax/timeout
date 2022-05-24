use std::convert::TryFrom;
use std::io;
use std::process::ExitStatus;
use std::process::{Child, Command};
use std::str::FromStr;
use std::thread;
use std::time::{Duration, Instant};

use libc::pid_t;
use nix::sys::signal::Signal;
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
    #[structopt(name = "duration", help = "e.g. 1s")]
    duration: String,

    #[structopt(name = "commands")]
    pub commands: Vec<String>,

    #[structopt(
        short = "s",
        long = "signal",
        default_value("SIGKILL"),
        help = "specify the signal to be sent on timeout"
    )]
    signal: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::from_args();

    let signal = match &args.signal.parse::<i32>() {
        Ok(n) => Signal::try_from(*n)?,
        Err(_) => Signal::from_str(&args.signal.to_uppercase())?,
    };
    let signal_number: libc::c_int = match signal.into() {
        Some(s) => s as libc::c_int,
        None => 0,
    };
    if signal_number == 0 {
        panic!("invalid signal number");
    }

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
            child.send_signal(signal_number)?;
        }
    };
    Ok(())
}

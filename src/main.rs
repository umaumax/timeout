use std::io;
use std::process::ExitStatus;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use clap::AppSettings;
use libc::pid_t;

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

fn build_app() -> clap::App<'static, 'static> {
    let program = std::env::args()
        .nth(0)
        .and_then(|s| {
            std::path::PathBuf::from(s)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap();

    clap::App::new(program)
        .about("simple timeoutcat command by rust")
        .version("0.0.1")
        .setting(clap::AppSettings::VersionlessSubcommands)
        .arg(
            clap::Arg::with_name("duration")
                .help("duration")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::with_name("command")
                .help("command")
                .required(true)
                .multiple(true)
                .index(2),
        )
        .setting(AppSettings::TrailingVarArg)
}

fn main() -> std::io::Result<()> {
    let matches = build_app().get_matches();
    let duration: Duration = {
        let text = matches.value_of("duration").unwrap();
        match text.parse::<f64>() {
            Ok(v) => Duration::from_secs_f64(v),
            _ => text.parse::<humantime::Duration>().unwrap().into(),
        }
    };
    let command_with_args: Vec<&str> = matches.values_of("command").unwrap().collect();
    let command = &command_with_args[0];
    let args = &command_with_args[1..];

    let mut child = Command::new(command).args(args).spawn().unwrap();

    match child.wait_or_timeout(duration).unwrap() {
        Some(status) => {
            if let Some(code) = status.code() {
                println!("code={}", code);
            } else {
                if cfg!(feature = "unix") {
                    println!("signal={}", status.signal().unwrap());
                }
                println!("signal");
            }
        }
        None => {
            child.send_signal(libc::SIGKILL)?;
        }
    };
    Ok(())
}

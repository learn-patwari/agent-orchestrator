//! Run a program with piped output, a scrubbed environment and a timeout. Shared by the Claude login provider
//! and the sandboxed shell/git tools. No shell is ever spawned: the caller passes an argument list.
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum RunError {
    Timeout(String),
    NotFound(String),
    Io(String),
}

/// (exit code, stdout, stderr). The environment is exactly `env`; nothing is inherited.
pub fn run(argv: &[String], cwd: Option<&Path>, env: &HashMap<String, String>, stdin: &str, timeout: f64) -> Result<(i32, String, String), RunError> {
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).env_clear().envs(env).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { RunError::NotFound(e.to_string()) } else { RunError::Io(e.to_string()) })?;
    let mut pipe = child.stdin.take().expect("piped");
    let input = stdin.to_string();
    std::thread::spawn(move || {
        let _ = pipe.write_all(input.as_bytes());
    });
    let (mut out, mut err) = (child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
    let o = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        String::from_utf8_lossy(&b).into_owned()
    });
    let e = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        String::from_utf8_lossy(&b).into_owned()
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed().as_secs_f64() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::Timeout(format!("timed out after {timeout} seconds")));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(RunError::Io(e.to_string())),
        }
    };
    Ok((status.code().unwrap_or(-1), o.join().unwrap_or_default(), e.join().unwrap_or_default()))
}

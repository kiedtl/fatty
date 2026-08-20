use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use marish::{
    vm::{self, StackValue},
    parser,
    compiler,
    ExitReason,
};

#[tokio::main]
async fn main() -> ExitCode {
    let mut stack_output = false;
    let mut tests = false;
    let mut script_path: Option<PathBuf> = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-s" | "--stack" => stack_output = true,
            "-t" | "--tests" => tests = true,
            other => script_path = Some(PathBuf::from(other)),
        }
    }

    let source = if let Some(p) = &script_path {
        match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("marish: {}: {e}", p.display());
                return ExitCode::FAILURE;
            }
        }
    } else {
        let mut buf = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
            eprintln!("marish: {e}");
            return ExitCode::FAILURE;
        }
        buf
    };

    let parsed = match parser::parse_str(&source) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("marish: {err}");
            return ExitCode::FAILURE;
        }
    };

    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let mut env: HashMap<_, _> = std::env::vars_os().collect();
    env.insert("FATTY".into(), "normal0".into());

    let program: Vec<_> = match compiler::compile(&path_dirs, &parsed) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("marish: {err:?}");
            return ExitCode::FAILURE;
        }
    };

    let (fd3_master, fd3_slave) = rustix::net::socketpair(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        rustix::net::SocketFlags::NONBLOCK,
        None,
    ).unwrap();

    let mut vm = vm::VM {
        test_ctx: Default::default(),
        fd3_slave: Some(std::sync::Arc::new(fd3_slave)),
        slave: None,
        program: Arc::new(program),
        pc: (0, None),
        stack: Vec::new(),
        scope: vec![vm::Scope::default()],
        rstack: Vec::new(),
        env: Arc::new(env),
        waiting_on: None,
        child_exit_stack: Vec::new(),
        status: None,
        msg_tx: None,
        done: false,
    };

    if tests {
        vm.pc.0 = vm.program.len() - 1;
        assert!(vm.program[vm.pc.0].id == vm::BlockId::TestsEntry);
    }

    //vm::print_program(&vm.program);
    vm.execute().await;

    let mut bwine_output = Vec::new();
    let mut buf = [0u8; 8192];
    let mut tries = 0;
    loop {
        match rustix::io::read(&fd3_master, &mut buf) {
            Err(rustix::io::Errno::AGAIN) => {
                tries += 1;
                if tries > 10 {
                    break;
                } else {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
            Ok(n) => bwine_output.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }

    if !bwine_output.is_empty() {
        let mut decoder = bwine::buffer_decoder(&bwine_output);
        let result = bwine::Value::read(&mut decoder).unwrap_or(bwine::Value::Undefined);
        println!("bwine:\n");
        if let Some(s) = result.os_string() {
            std::io::stdout().write_all(s.as_bytes()).unwrap();
        } else {
            println!("{:?}", result);
        }
    }


    if stack_output && !vm.stack.is_empty() {
        eprintln!("-- final stack --");
        for (i, sv) in vm.stack.iter().enumerate() {
            match sv {
                StackValue::Value(v) => eprintln!("[{i}] {v:?}"),
                StackValue::VarRef(r) => eprintln!("[{i}] <varref {r:?}>"),
            }
        }
    }

    match vm.child_exit_stack.last() {
        Some(reason) => exit_code_from_reason(reason),
        None => ExitCode::SUCCESS,
    }
}

fn exit_code_from_reason(reason: &ExitReason) -> ExitCode {
    let code: u8 = match reason {
        ExitReason::Normal(code) => (*code & 0xff) as u8,
        ExitReason::Signal { signal, .. } => {
            let signum = signal.as_raw();
            (128u32.saturating_add(signum as u32).min(255)) as u8
        }
        ExitReason::Unknown { .. } => 1,
    };
    ExitCode::from(code)
}

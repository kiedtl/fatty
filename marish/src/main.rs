use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use marish::{
    vm::{self, StackValue},
    parser,
    compiler,
    ExitReason,
};

fn main() -> ExitCode {
    let mut stack_output = false;
    let mut script_path: Option<PathBuf> = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-s" | "--stack" => stack_output = true,
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
    let env: HashMap<_, _> = std::env::vars_os().collect();

    let program: Vec<_> = match compiler::compile(&path_dirs, &parsed) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("marish: {err:?}");
            return ExitCode::FAILURE;
        }
    };

    let mut vm = vm::VM {
        fd3_slave: None,
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

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("marish: tokio: {e}");
            return ExitCode::FAILURE;
        }
    };
    rt.block_on(vm.execute());

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

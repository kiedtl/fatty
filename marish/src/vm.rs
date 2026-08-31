use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, PipeReader};
use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;
use std::sync::Arc;

use crate::ExitReason;
use crate::parser::*;
use crate::compiler;
use crate::utils::FdRw;

use bwine::Value;
use futures::future::{FutureExt, Shared, BoxFuture};
use rustix::process::Pid;
use rustix::fd::{AsFd, AsRawFd, OwnedFd, BorrowedFd};
use tokio::sync::{mpsc, watch};

// pub enum RunCondition {
//     PreviousFailed,
//     PreviousSucceeded,
// }

#[derive(Debug, Clone, PartialEq)]
pub struct Command2 {
    pub lc: LineCol,
    pub path: Option<PathBuf>,
    pub orig: String,
    pub argc: usize, // 0 means no arguments.
}

#[derive(Debug, Clone)]
pub enum RunPipelineItem {
    Command(Command2),
    Where { block: usize, lc: LineCol },
    // Query(Query),
}

#[derive(Debug, Clone)]
pub enum Instr {
    EnterTest(String),
    RecordTestSuccess,
    Assert(LineCol),

    ChangeDir,

    /// Execute command in current context
    Run {
        command: Command2,
        muffle: bool,
        // cond: RunCondition,
    },

    /// Execute pipeline in current context
    RunPipeline {
        items: Vec<RunPipelineItem>,
        is_there_initial_value: bool,
        muffle: bool,
        // cond: RunCondition,
    },

    /// Where block. Currently allowed only in pipelines (RunPipelineItem), so commented out here.
    // Where { func: usize },

    /// Execute block in new async context
    CallAsync { block: usize },

    /// Execute block in current context
    Call { block: usize },

    /// Push a non-Table/Array/Var Token to the stack.
    Load(Token),

    /// Instructions to load a variable. VarRef starts out by pushing a VarRef with an empty set of
    /// fields; GetColumn/Index adds fields, and Deref/SetVar finalizes it by either retrieving the
    /// value or setting the variable
    VarRef(String),
    GetColumn,
    GetIndex,
    Deref,
    SetVar,

    /// Collect the top N stack elements into a single Value::Array, pushing it.
    CollectArray(usize),

    /// Collect the top N stack elements, assumed to be Value::Array, plus an additional
    /// Value::Array for the header, into a single Value::Table { header, rows }
    CollectTable(usize),

    /// Execute an operator on the top two elements of the stack.
    Op(Operator),

    /// Negate the TOS
    Negate,

    /// Pop the TOS
    Pop,

    /// Pop from rstack, returning if possible or otherwise marking the VM as "done"
    Return,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockId {
    Entry,
    TestsEntry,
    Test { name: String },
    Internal,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub id: BlockId,
    pub contents: Vec<Instr>,
}

impl Block {
    pub fn new_internal() -> Self {
        Self {
            id: BlockId::Internal,
            contents: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct Job {
    pub status: watch::Receiver<VMStatus>,
}

impl Job {
    pub fn is_done(&self) -> bool {
        matches!(&*self.status.borrow(), VMStatus::Resolved { .. })
    }
}

#[derive(Default, Clone)]
pub enum VMStatus {
    #[default]
    None,
    Waiting {
        item: RunPipelineItem,
        on: WaitingOn2,
    },
    Done {
        command: Option<Command2>,
        reason: ExitReason,
    },
    Resolved {
        command: Option<Command2>,
        reason: Option<ExitReason>,
    }
}

#[derive(Clone)]
pub enum VMMessage {
    ChangedDir,
    Done {
        last_exit_reason: Option<ExitReason>,
        stack: Vec<StackValue>,
        vars: HashMap<String, Value<'static>>,
    },
    Waiting(WaitingOn2),
    Job(Job),
}

pub enum WaitingOn {
    Pid(Pid, Option<PipeReader>),
    Builtin(Shared<BoxFuture<'static, ()>>, Option<PipeReader>),
}

#[derive(Clone, Debug)]
pub enum WaitingOn2 {
    Pid(Pid),
    Builtin(tokio::task::AbortHandle),
}

#[derive(Default, Debug, Clone)]
pub struct Scope {
    pub vars: HashMap<String, Value<'static>>,
    pub is_inherited: bool,
}

#[derive(Debug, Clone)]
pub struct VarRef {
    name: String,
    fields: Vec<Field>,
}

impl VarRef {
    pub fn new(name: &str) -> Self {
        let name = name.to_owned();
        Self { name, fields: Vec::new() }
    }
}

#[derive(Debug, Clone)]
pub enum Field {
    Column(Value<'static>),
    Index(Value<'static>),
}

#[derive(Debug, Clone)]
pub enum StackValue {
    Value(Value<'static>),
    VarRef(VarRef),
}

#[derive(Debug, Default)]
pub struct TestCtx {
    current_test: Option<String>,
    successful: usize,
    failed: usize,
}

pub struct VM {
    pub test_ctx: TestCtx,

    pub fd3_slave: Option<Arc<OwnedFd>>,
    pub slave: Option<Arc<OwnedFd>>,
    pub program: Arc<Vec<Block>>,
    pub pc: (usize, Option<usize>),

    pub stack: Vec<StackValue>,
    pub scope: Vec<Scope>,
    pub rstack: Vec<(usize, Option<usize>)>,

    pub env: Arc<HashMap<OsString, OsString>>,
    pub waiting_on: Option<WaitingOn>,
    pub child_exit_stack: Vec<ExitReason>,

    // For communicating between top-level shell and subshells. Rx is inside App.jobs.
    pub status: Option<watch::Sender<VMStatus>>,

    // For communicating between top-level shell and Fatty.
    pub msg_tx: Option<mpsc::Sender<VMMessage>>,

    pub done: bool,
}

macro_rules! pop_value {
    ($s:expr) => {
        match $s.stack.pop().unwrap() {
            StackValue::Value(value) => value,
            StackValue::VarRef(var) => panic!("expected value, got varref: {var:?}"),
        }
    }
}

impl VM {
    fn push_value(&mut self, v: Value<'static>) {
        self.stack.push(StackValue::Value(v));
    }

    // Explicitly write out result type, and do the Box::pin thing because execute calls
    // execute_once which calls execute which calls execute_once... which makes rustc give up on
    // deciding whether execute_once() is Send or not, which makes tokio:spawn() very sad.
    pub fn execute(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            loop {
                self.execute_once().await;
                if self.done {
                    break;
                }

                let mut reader = None;
                let mut reason = ExitReason::Builtin;
                match self.waiting_on.take() {
                    None => (),
                    Some(WaitingOn::Builtin(jh, maybe_reader)) => {
                        reader = maybe_reader;
                        jh.await;
                    },
                    Some(WaitingOn::Pid(pid, maybe_reader)) => {
                        reader = maybe_reader;
                        match rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty()) {
                            Ok(Some((_, status))) => {
                                reason = ExitReason::from(status);
                                self.child_exit_stack.push(reason);
                            },
                            Ok(None) | Err(_) => unreachable!(),
                        }
                    }
                }

                if let Some(sender) = &self.status {
                    sender.send_modify(move |previous| {
                        let command = match std::mem::take(previous) {
                            VMStatus::Waiting { item: RunPipelineItem::Command(command), .. } => Some(command),
                            VMStatus::Waiting { .. } => None,
                            _ => unreachable!(),
                        };
                        *previous = VMStatus::Done { command, reason };
                    });
                }

                if let Some(mut reader) = reader {
                    let mut output = Vec::new();
                    loop {
                        let mut buf = [0u8; 8192];
                        match reader.read(&mut buf) {
                            Err(_) | Ok(0) => break,
                            Ok(n) => output.extend_from_slice(&buf[..n]),
                        }
                    }

                    let mut decoder = bwine::buffer_decoder(&output);
                    let result = Value::read(&mut decoder).unwrap_or(Value::Undefined);
                    self.push_value(result);
                }
            }
        })
    }

    pub async fn execute_once(&mut self) {
        assert!(!self.done);
        assert!(self.waiting_on.is_none());

        // Increment PC
        let instr_pc = match self.pc.1 {
            None => 0,
            Some(c) => c + 1,
        };
        self.pc.1 = Some(instr_pc);

        match &self.program[self.pc.0].contents[instr_pc] {
            Instr::EnterTest(s) => {
                if self.test_ctx.current_test.is_some() {
                    panic!("Already in test");
                }
                self.test_ctx.current_test = Some(s.clone());
            },
            Instr::RecordTestSuccess => {
                println!("{: <50} \x1b[1;34mPASS\x1b[m", self.test_ctx.current_test.as_ref().unwrap());
                self.test_ctx.current_test = None;
                self.test_ctx.successful += 1;
            },
            Instr::Assert(lc) => {
                let value = pop_value!(self);
                if value != Value::Bool(true) {
                    if let Some(curr) = &self.test_ctx.current_test {
                        println!("{: <50} \x1b[1;31mFAIL\x1b[m", curr);
                        self.test_ctx.current_test = None;
                        self.ret().await;
                        self.test_ctx.failed += 1;
                    } else {
                        panic!("Assertion failed {lc:?} ({value:?})");
                    }
                }
            },
            Instr::ChangeDir => {
                let path = pop_value!(self).to_string();
                if let Err(_e) = std::env::set_current_dir(&path) {
                    //outln!("cd: {e:?}");
                }

                if let Some(sender) = &self.msg_tx {
                    sender.send(VMMessage::ChangedDir).await.unwrap();
                }
            }
            Instr::Run { muffle, command } => {
                let Some(args) = (0..command.argc)
                    .map(|_| pop_value!(self))
                    .map(|v| v.os_string())
                    .collect::<Option<Vec<OsString>>>()
                    else {
                        panic!("One of the arguments cannot be turned into a string");
                    };

                let mut muffle_reader = None;
                let mut keep: Option<OwnedFd> = None;

                let command_path = command.path
                    .clone()
                    .unwrap_or_else(|| compiler::resolve(&compiler::path_from_env(&self.env), &command.orig).unwrap());
                let mut pcmd = std::process::Command::new(&command_path);
                pcmd.envs(&*self.env);
                pcmd.args(args);

                if !muffle && let Some(fd3_slave) = &self.fd3_slave {
                    add_stdobjout(&mut pcmd, fd3_slave.as_fd());
                }

                if *muffle {
                    let (fr, fw) = std::io::pipe().unwrap();
                    muffle_reader = Some(fr);
                    add_stdobjout(&mut pcmd, fw.as_fd());
                    keep = Some(fw.into()); // Keep alive in memory until after spawn() is called
                } else if let Some(slave) = &self.slave {
                    let slave = slave.as_fd();
                    pcmd.stdin(rustix::io::dup(slave).unwrap());
                    pcmd.stdout(rustix::io::dup(slave).unwrap());
                    pcmd.stderr(rustix::io::dup(slave).unwrap());
                    add_terminal_controller(&mut pcmd, slave);
                }

                let child = pcmd.spawn().unwrap();
                std::mem::drop(keep);

                let on = WaitingOn::Pid(Pid::from_child(&child), muffle_reader);
                self.waiting_on = Some(on);

                if let Some(sender) = &self.msg_tx {
                    let on = WaitingOn2::Pid(Pid::from_child(&child));
                    sender.send(VMMessage::Waiting(on)).await.unwrap();
                }

                if let Some(sender) = &self.status {
                    let on = WaitingOn2::Pid(Pid::from_child(&child));
                    sender.send(VMStatus::Waiting {
                        on,
                        item: RunPipelineItem::Command(command.clone())
                    }).unwrap();
                }
            },
            Instr::RunPipeline { muffle, items, is_there_initial_value } => {
                let mut reader;
                let mut next_reader = None;
                let mut writer = None;

                let mut reader_obj;
                let mut next_reader_obj = None;
                let mut writer_obj = None;

                let mut last_one = None;

                if *is_there_initial_value {
                    let (fr_obj, fw_obj) = std::io::pipe().unwrap();
                    next_reader_obj = Some(fr_obj);
                    let v = pop_value!(self);
                    let mut wr = bwine::generic_writer(&fw_obj);
                    v.write(&mut wr).unwrap();
                }

                for (i, item) in items.iter().enumerate() {
                    // Keep holds fds until Command::spawn() is called
                    let mut keep = Vec::<OwnedFd>::new();

                    reader = next_reader;
                    next_reader = None;

                    reader_obj = next_reader_obj;
                    next_reader_obj = None;

                    if i < items.len() - 1 {
                        let (fr, fw) = std::io::pipe().unwrap();
                        next_reader = Some(fr);
                        writer = Some(fw);

                        let (fr_obj, fw_obj) = std::io::pipe().unwrap();
                        next_reader_obj = Some(fr_obj);
                        writer_obj = Some(fw_obj);
                    }

                    let mut muffle_reader = None;
                    if *muffle && i == items.len() - 1 {
                        let (fr, fw) = std::io::pipe().unwrap();
                        muffle_reader = Some(fr);
                        assert!(writer_obj.is_none());
                        writer_obj = Some(fw);
                    }

                    match item {
                        RunPipelineItem::Where { block, .. } => {
                            let writer = writer_obj.take()
                                .map(|w| Box::new(w) as Box<dyn std::io::Write + Send>)
                                .or_else(|| self.fd3_slave.as_ref().map(|fd3| Box::new(FdRw(fd3.clone())) as _))
                                .unwrap();
                            let reader = reader_obj.take()
                                .map(|r| Box::new(r) as Box<dyn std::io::Read + Send>)
                                .or_else(|| self.fd3_slave.as_ref().map(|fd3| Box::new(FdRw(fd3.clone())) as _))
                                .unwrap();
                            let program = self.program.clone();
                            let block = *block;
                            let mut scope = self.scope.clone();
                            // TODO: smart scope cloning -- crush into single scope, and only clone
                            // what's needed
                            for scope in &mut scope {
                                scope.is_inherited = true;
                            }
                            let mut vm = VM {
                                test_ctx: Default::default(),
                                fd3_slave: self.fd3_slave.clone(),
                                slave: self.slave.clone(),
                                env: self.env.clone(),
                                program,
                                stack: Vec::new(),
                                scope,
                                rstack: Vec::new(),
                                pc: (0, None),
                                waiting_on: None,
                                child_exit_stack: Vec::new(),
                                status: None,
                                msg_tx: None,
                                done: false,
                            };
                            let jh = tokio::spawn(async move {
                                builtin::filter(&mut vm, block, reader, writer).await.unwrap();
                            });
                            let ah = jh.abort_handle();
                            let shjh = async move { jh.await.unwrap(); }.boxed().shared();
                            last_one = Some((WaitingOn::Builtin(shjh, muffle_reader), WaitingOn2::Builtin(ah)));
                        },
                        RunPipelineItem::Command(command) => {
                            let Some(args) = (0..command.argc)
                                .map(|_| pop_value!(self))
                                .map(|v| v.os_string())
                                .collect::<Option<Vec<_>>>()
                                else {
                                    panic!("One of the arguments cannot be turned into a string");
                                };

                            let command_path = command.path
                                .clone()
                                .unwrap_or_else(|| compiler::resolve(&compiler::path_from_env(&self.env), &command.orig).unwrap());
                            let mut command = std::process::Command::new(&command_path);
                            command.envs(&*self.env);
                            command.args(args);

                            if let Some(writer) = writer.take() {
                                command.stdout(writer);
                            } else if let Some(slave) = &self.slave {
                                command.stdout(rustix::io::dup(slave.as_fd()).unwrap());
                            }

                            if let Some(writer_obj) = writer_obj.take() {
                                add_stdobjout(&mut command, writer_obj.as_fd());
                                keep.push(writer_obj.into());
                            } else if let Some(fd3_slave) = &self.fd3_slave {
                                add_stdobjout(&mut command, fd3_slave.as_fd());
                            }

                            if let Some(reader) = reader {
                                command.stdin(reader);
                            } else if let Some(slave) = &self.slave {
                                command.stdin(rustix::io::dup(slave.as_fd()).unwrap());
                            }

                            if let Some(reader_obj) = reader_obj {
                                add_stdobjin(&mut command, reader_obj.as_fd());
                                keep.push(reader_obj.into());
                            } else if let Some(fd3_slave) = &self.fd3_slave {
                                add_stdobjin(&mut command, fd3_slave.as_fd());
                            }

                            if let Some(slave) = &self.slave {
                                command.stderr(rustix::io::dup(slave.as_fd()).unwrap());
                            }

                            let child = command.spawn().unwrap();
                            last_one = Some((
                                    WaitingOn::Pid(Pid::from_child(&child), muffle_reader),
                                    WaitingOn2::Pid(Pid::from_child(&child))
                            ));
                            std::mem::drop(keep);
                        }
                    }
                }

                let (wo, wo2) = last_one.unwrap();
                self.waiting_on = Some(wo);
                if let Some(sender) = &self.status {
                    sender.send(VMStatus::Waiting {
                        item: items.last().unwrap().clone(),
                        on: wo2,
                    }).unwrap();
                }
            },
            Instr::CallAsync { block } => {
                let (tx, rx) = watch::channel(VMStatus::default());

                let env = self.env.clone();
                let program = self.program.clone();
                let block = *block;
                let mut scope = self.scope.clone();
                // TODO: smart scope cloning -- crush into single scope, and only clone what's
                // needed
                for scope in &mut scope {
                    // TODO: differentiate between scope inherited by non-spawned child VM and
                    // spawned Job VM -- i.e. one is const and one isn't
                    scope.is_inherited = true;
                }
                let mut vm = VM {
                    test_ctx: Default::default(),
                    fd3_slave: self.fd3_slave.clone(),
                    slave: None,
                    env,
                    program,
                    stack: Vec::new(),
                    scope,
                    rstack: Vec::new(),
                    pc: (block, None),
                    waiting_on: None,
                    child_exit_stack: Vec::new(),
                    status: Some(tx),
                    msg_tx: None,
                    done: false,
                };
                tokio::spawn(async move {
                    vm.execute().await;
                });

                if let Some(sender) = &self.msg_tx {
                    sender.send(VMMessage::Job(Job { status: rx, })).await.unwrap();
                }
            },
            Instr::Call { block } => {
                self.rstack.push(self.pc);
                self.scope.push(Scope::default());
                self.pc = (*block, None);
            },
            Instr::GetColumn => {
                let key = pop_value!(self);
                let Some(StackValue::VarRef(mut varref)) = self.stack.pop() else { unreachable!() };
                varref.fields.push(Field::Column(key));
                self.stack.push(StackValue::VarRef(varref));
            }
            Instr::GetIndex => {
                let key = pop_value!(self);
                let Some(StackValue::VarRef(mut varref)) = self.stack.pop() else { unreachable!() };
                varref.fields.push(Field::Index(key));
                self.stack.push(StackValue::VarRef(varref));
            }
            Instr::SetVar => {
                let Some(StackValue::VarRef(varref)) = self.stack.pop() else { unreachable!() };
                let value = pop_value!(self);
                for scope in self.scope.iter_mut().rev() {
                    if let Some(var) = scope.vars.get_mut(&varref.name) {
                        assert!(!scope.is_inherited); // TODO: external scopes, SetVar messages across VMs
                        setv(&varref.fields, N::V(var), value);
                        return;
                    }
                }
                self.scope.last_mut().unwrap().vars.insert(varref.name, value);
            },
            Instr::CollectArray(n) => {
                let mut accm = Vec::new();
                for _ in 0..*n {
                    accm.push(pop_value!(self));
                }
                self.push_value(Value::Array(accm.into()));
            },
            Instr::CollectTable(n) => {
                let mut rows = Vec::new();
                for _ in 0..*n {
                    let Value::Array(row) = pop_value!(self)
                        else { unreachable!() };
                    rows.push(row.into());
                }

                let Value::Array(header) = pop_value!(self)
                    else { unreachable!() };
                let header = header.into();

                self.push_value(Value::Table { header, rows });
            },
            Instr::Deref => {
                let Some(StackValue::VarRef(varref)) = self.stack.pop() else { unreachable!() };
                let mut value = self.get_var(&varref.name).unwrap().clone();
                for field in varref.fields {
                    match field {
                        Field::Column(key) => {
                            match value {
                                Value::Table { header, rows } => {
                                    if let Some(i) = header.iter().position(|c| *c == key) {
                                        value = Value::Array(
                                            rows.into_iter()
                                                .map(|mut r| r.remove(i))
                                                .collect::<Vec<_>>()
                                                .into()
                                        );
                                    } else {
                                        //panic!("No such column {key:?} on table({header:?}).");
                                        value = Value::Array((&[Value::Null]).into());
                                    }
                                }
                                _ => panic!("invalid target for column index: {value:?}"),
                            }
                        }
                        Field::Index(key) => {
                            let i = match key {
                                Value::Int(i) => i as usize,
                                Value::Float(f) => {
                                    let i = f as usize;
                                    assert!(f as f64 == f);
                                    i
                                }
                                c => panic!("index must be an integer, got {c:?}"),
                            };
                            value = match value {
                                Value::Array(arr) => arr.get(i).unwrap_or(&Value::Null).clone(),
                                Value::Table { rows, .. } => rows.get(i)
                                    .map(|i| Value::Array(i.clone().into()))
                                    .unwrap_or(Value::Null),
                                _ => panic!("invalid target for row index: {value:?}"),
                            };
                        }
                    }
                }
                self.push_value(value);
            }
            Instr::VarRef(name) => {
                self.stack.push(StackValue::VarRef(VarRef::new(&name)));
            }
            Instr::Load(tok) => {
                match tok {
                    Token::Word(s) => for item in expand_token(&s) {
                        self.push_value(Value::from(item));
                    },
                    Token::String(v) => self.push_value(Value::from(v.clone())),
                    Token::Int(v) => self.push_value(Value::Int(*v)),
                    Token::Float(v) => self.push_value(Value::Float(*v)),
                    Token::Var(_) => unreachable!(),
                    Token::Array(_) => unreachable!(),
                    Token::Table(_) => unreachable!(),
                }
            },
            Instr::Op(op) => {
                fn cmp<T: PartialOrd>(a: T, b: T, op: Operator) -> bool {
                    match op {
                        Operator::Gt => a > b,
                        Operator::Lt => a < b,
                        Operator::Le => a <= b,
                        Operator::Ge => a >= b,
                        _ => unreachable!(),
                    }
                }

                fn math<T: num_traits::Num>(a: T, b: T, op: Operator) -> T {
                    match op {
                        Operator::Add => a + b,
                        Operator::Sub => a - b,
                        Operator::Mul => a * b,
                        Operator::Div => a / b,
                        _ => unreachable!(),
                    }
                }

                let b = pop_value!(self);
                let a = pop_value!(self);

                let op = *op;
                let result = match op {
                    Operator::Eq => Value::Bool(a == b),
                    Operator::Ne => Value::Bool(a != b),
                    Operator::Gt | Operator::Lt | Operator::Le | Operator::Ge
                        => Value::Bool(
                            match (&a, &b) {
                                (Value::Int(a), Value::Int(b)) => cmp(*a, *b, op),
                                (Value::Float(a), Value::Float(b)) => cmp(*a, *b, op),
                                (Value::Int(a), Value::Float(b)) => cmp(*a as f64, *b, op),
                                (Value::Float(a), Value::Int(b)) => cmp(*a, *b as f64, op),
                                (Value::Text(a), Value::Text(b)) => cmp(a, b, op),
                                _ => panic!("invalid operands: {:?} and {:?}", a, b),
                            }
                        ),
                    Operator::And | Operator::Or | Operator::Xor
                        => match (a, b) {
                            (Value::Int(_), Value::Int(_)) => todo!(), // binary operators?
                            (Value::Bool(a), Value::Bool(b)) => Value::Bool(match op {
                                Operator::And => a && b,
                                Operator::Or => a || b,
                                Operator::Xor => a ^ b,
                                _ => unreachable!(),
                            }),
                            _ => panic!("invalid operands"),
                        }
                    Operator::Add | Operator::Sub | Operator::Mul | Operator::Div
                        => match (&a, &b) {
                            (Value::Text(a), Value::Text(b)) if op == Operator::Add => {
                                let r = format!("{}{}", a.to_string(), b.to_string());
                                Value::from(r)
                            },
                            (Value::Int(a), Value::Int(b)) => Value::Int(math(*a, *b, op)),
                            (Value::Float(a), Value::Float(b)) => Value::Float(math(*a, *b, op)),
                            (Value::Int(a), Value::Float(b)) => Value::Float(math(*a as f64, *b, op)),
                            (Value::Float(a), Value::Int(b)) => Value::Float(math(*a, *b as f64, op)),
                            _ => panic!("invalid operands: {:?} and {:?}", a, b),
                        },
                    Operator::Like => todo!(),
                    Operator::NotLike => todo!(),
                };

                self.push_value(result);
            }
            Instr::Negate => {
                let res = !pop_value!(self).truthy();
                self.push_value(Value::Bool(res));
            }
            Instr::Pop => {
                _ = self.stack.pop();
            }
            Instr::Return => {
                self.ret().await;
            },
        }
    }

    async fn ret(&mut self) {
        if let Some(newpc) = self.rstack.pop() {
            self.pc = newpc;
            self.scope.pop().unwrap();
        } else {
            self.done = true;
            assert!(self.scope.len() >= 1);
            if let Some(sender) = &self.msg_tx {
                sender.send(VMMessage::Done {
                    stack: std::mem::take(&mut self.stack),
                    last_exit_reason: self.child_exit_stack.pop(),
                    vars: self.scope.last().unwrap().vars.clone(),
                }).await.unwrap();
            }
            if let Some(sender) = &self.status {
                sender.send_modify(move |previous| {
                    match std::mem::take(previous) {
                        VMStatus::Done { command, reason } => *previous = VMStatus::Resolved { command, reason: Some(reason) },
                        VMStatus::None => *previous = VMStatus::Resolved { command: None, reason: None },
                        VMStatus::Resolved { command, reason } => panic!("Job already resolved: {command:?} {reason:?}"),
                        VMStatus::Waiting { item, on } => panic!("Job waiting: {item:?} {on:?}"),
                    }
                });
            }
        }
    }

    fn get_var(&self, s: &str) -> Option<&Value<'static>> {
        match s {
            "t" => return Some(&Value::Bool(true)),
            "f" => return Some(&Value::Bool(false)),
            "nil" => return Some(&Value::Null),
            "undef" => return Some(&Value::Undefined),
            _ => (),
        }
        for scope in self.scope.iter().rev() {
            if let Some(value) = scope.vars.get(s) {
                return Some(value);
            }
        }
        None
    }
}

#[derive(Debug)]
enum N<'a, 'v> {
    V(&'a mut Value<'v>),
    A(&'a mut [Value<'v>]),
    T {
        header: &'a [Value<'v>],
        row: &'a mut Vec<Value<'v>>,
    },
    C {
        col: usize,
        rows: &'a mut [Vec<Value<'v>>],
    }
}

fn setv<'a>(mut fields: &[Field], lhs: N<'a, 'static>, rhs: Value<'static>) {
    // Get the latest field and take a look at it, UNLESS fields is empty, in which case
    // split_off_first returns None and we set the variable
    let Some(field) = fields.split_off_first() else {
        match (lhs, rhs) {
            (N::V(v), rhs) => *v = rhs.clone(),
            (N::A(arr), Value::Array(rhs)) => {
                if arr.len() != rhs.len() {
                    panic!("setv(N::A): rhs len doesn't match: {} vs lhs' {}", rhs.len(), arr.len());
                }
                for (lhs, rhs) in arr.iter_mut().zip(rhs.into_iter()) {
                    *lhs = rhs.clone();
                }
            }
            (N::A(_), rhs) => panic!("setv(N::A): rhs is wrong shape ({:?})", rhs),
            (N::C { rows, .. }, _) => {
                assert!(rows.len() != 1);
                panic!("setv(N::C): cannot set column for table.");
            },
            // Simplify N::T into N::A
            (N::T { row, .. }, rhs) => setv(&[], N::A(row), rhs),
        }
        return;
    };

    match field {
        Field::Column(key) => {
            match lhs {
                N::T { header, row } => {
                    let Some(col) = header.iter().position(|c| c == key) else {
                        panic!("No such column {key:?} on table({header:?}).");
                    };
                    setv(fields, N::V(&mut row[col]), rhs);
                }
                N::V(Value::Table { header, rows }) => {
                    let Some(col) = header.iter().position(|c| c == key) else {
                        panic!("No such column {key:?} on table({header:?}).");
                    };
                    setv(fields, N::C { col, rows }, rhs);
                }
                _ => panic!("invalid target for column index: {lhs:?}"),
            }
        }
        Field::Index(key) => {
            let i = match key {
                Value::Int(i) => *i as usize,
                Value::Float(f) => {
                    let i = *f as usize;
                    assert!(i as f64 == *f);
                    i
                }
                _ => panic!("index must be an integer"),
            };
            match lhs {
                N::V(Value::Array(arr)) => setv(fields, N::V(&mut arr.to_mut()[i]), rhs),
                N::V(Value::Table { header, rows }) => setv(fields, N::T { header, row: &mut rows[i] }, rhs),
                N::A(arr) => setv(fields, N::V(&mut arr[i]), rhs),
                N::C { col, rows } => setv(fields, N::V(&mut rows[i][col]), rhs),
                _ => panic!("invalid target for row index: {lhs:?}"),
            }
        }
    }
}

#[allow(dead_code)]
pub fn print_program(p: &[Block]) {
    for (blocki, block) in p.iter().enumerate() {
        println!("Block {blocki}-{:?}:", block.id);
        for instr in &block.contents {
            match instr {
                Instr::EnterTest(name) => println!("  - enter_test {name}"),
                Instr::RecordTestSuccess => println!("  - record_test_success"),
                Instr::Assert(_) => println!("  - assert"),
                Instr::ChangeDir => println!("  - cd"),
                Instr::Run { command: Command2 { path, argc, .. }, muffle }
                    => println!("  - run muffle={muffle} {:?} ({argc} args)", path),
                Instr::RunPipeline { items, muffle, is_there_initial_value }
                    => {
                        println!("  - create_pipe muffle={muffle} iv={is_there_initial_value}");
                        for item in items {
                            match item {
                                RunPipelineItem::Command(c) => println!("  - run {:?}: ({} args)", c.path, c.argc),
                                RunPipelineItem::Where { block, .. } => println!("  - where {block}"),
                            }
                        }
                    },
                Instr::CallAsync { block } => println!("  - call_async {block}"),
                Instr::SetVar => println!("  - set_var"),
                Instr::Call { block } => println!("  - call {block}"),
                Instr::Load(tok) => println!("  - push {}", tok.to_string()),
                Instr::VarRef(s) => println!("  - load {s}"),
                Instr::GetColumn => println!("  - index_column"),
                Instr::GetIndex => println!("  - index_row"),
                Instr::Deref => println!("  - deref"),
                Instr::CollectArray(n) => println!("  - collect_array {n}"),
                Instr::CollectTable(n) => println!("  - collect_table {n}"),
                Instr::Op(op) => println!("  - op {op:?}"),
                Instr::Negate => println!("  - negate"),
                Instr::Pop => println!("  - pop"),
                Instr::Return => println!("  - return"),
            }
        }
    }
}

unsafe fn safe_dup_nocloexec(fdno: i32, to: i32) -> std::io::Result<()> {
    unsafe {
        if fdno == to {
            // fd is already TO, clear CLOEXEC
            let flags = libc::fcntl(to, libc::F_GETFD);
            if flags == -1 || libc::fcntl(to, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                return Err(std::io::Error::last_os_error());
            }
        } else {
            // Silly rustix requires an OwnedFd, so use libc.
            if libc::dup2(fdno, to) == -1 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

fn add_stdobjin(cmd: &mut std::process::Command, fd3_slave: BorrowedFd) {
    let fd3_raw = fd3_slave.as_raw_fd();
    unsafe {
        cmd.pre_exec(move || safe_dup_nocloexec(fd3_raw, 4));
    }
}

fn add_stdobjout(cmd: &mut std::process::Command, fd3_slave: BorrowedFd) {
    let fd3_raw = fd3_slave.as_raw_fd();
    unsafe {
        cmd.pre_exec(move || safe_dup_nocloexec(fd3_raw, 3));
    }
}

fn add_terminal_controller(cmd: &mut std::process::Command, slave: BorrowedFd) {
    let raw = slave.as_raw_fd();
    unsafe {
        cmd.pre_exec(move || {
            rustix::process::setsid()?;
            rustix::process::ioctl_tiocsctty(&BorrowedFd::borrow_raw(raw))?;
            Ok(())
        });
    }
}

fn expand_token(token: &str) -> Vec<String> {
    let token = expand_tilde(token);
    expand_glob(&token)
}

fn expand_tilde(token: &str) -> String {
    if token == "~" || token.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}{}", home.to_string_lossy(), &token[1..]);
        }
    }
    token.to_owned()
}

fn expand_glob(token: &str) -> Vec<String> {
    // Only bother if it looks like a glob
    if !token.contains(['*', '?', '[']) {
        return vec![token.to_owned()];
    }

    let matches: Vec<String> = glob::glob(token)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    println!("expanded {} into {:?}", token, matches);

    // bash behaviour: if no match, pass the literal token through
    if matches.is_empty() { vec![token.to_owned()] } else { matches }
}

mod builtin {
    use super::*;

    use std::io::Read;
    use anyhow::Result;
    use bwine::{self, Token};

    pub async fn filter(
        vm: &mut VM,
        block: usize,
        mut reader: Box<dyn std::io::Read + Send>,
        writer: Box<dyn std::io::Write + Send>
    ) -> Result<()> {
        let mut sr = bwine::StreamingReader::new();
        let mut buf = Vec::<u8>::new();
        let mut ast = Vec::new();
        let mut consumed = 0;

        let mut writer = bwine::minicbor::encode::Encoder::new(
            bwine::minicbor::encode::write::Writer::new(writer)
        );

        enum S<'a, W: bwine::minicbor::encode::Write> {
            V,
            H,
            PT(Vec<Value<'static>>, bwine::StreamingTable<'a, W>),
            PA(bwine::StreamingArray<'a, W>),
        }
        let mut s = S::V;
        let mut ai = 0;

        while !sr.is_done() {
            let mut tmp = [0u8; 1024];
            let n = reader.read(&mut tmp)?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[0..n]);

            while !sr.is_done() {
                match sr.read_once(&buf[consumed..], &mut ast) {
                    Ok(nn) => consumed += nn,
                    Err(e) if e.is_end_of_input() => break,
                    Err(e) => {
                        eprintln!("Error: {e:?}");
                        return Ok(());
                    },
                }

                if ai < ast.len() {
                    match &mut s {
                        S::V => {
                            match &ast[ai] {
                                Token::Table => s = S::H,
                                Token::Array(_) => {
                                    std::mem::drop(s);
                                    s = S::PA(bwine::stream_array(&mut writer, None).unwrap());
                                },
                                _ => {
                                    //outln!("Expected table or array.");
                                    return Ok(());
                                }
                            }
                            ai += 1;
                        }
                        S::H => {
                            if let Token::Array(_) = &ast[ai]
                                && let Some((ns, Value::Array(harr))) = Token::collect(&ast[ai..])
                            {
                                ai += ns + 1; // Skip next Token::Array that begins rows.
                                let harr = harr.into_owned();
                                std::mem::drop(s);
                                let stt = bwine::stream_table(&mut writer, harr.clone()).unwrap();
                                s = S::PT(harr.clone(), stt);
                            }
                        }
                        S::PT(headers, stt) => {
                            if let Token::Array(_) = &ast[ai]
                                && let Some((ns, Value::Array(row))) = Token::collect(&ast[ai..])
                            {
                                vm.done = false;
                                vm.stack.clear();
                                vm.pc = (block, None);
                                vm.scope.push(Scope {
                                    is_inherited: false,
                                    vars: [("_".to_owned(), Value::Table {
                                        header: headers.clone(),
                                        rows: vec![row.clone().into_owned()]
                                    })].into_iter().collect(),
                                });
                                vm.execute().await;
                                if pop_value!(vm) == Value::Bool(true) {
                                    stt.row(row.into_owned()).unwrap();
                                }

                                ai += ns;
                            }
                        },
                        S::PA(sta) => {
                            if let Some((ns, value)) = Token::collect(&ast[ai..]) {
                                vm.done = false;
                                vm.stack.clear();
                                vm.pc = (block, None);
                                vm.scope.push(Scope {
                                    is_inherited: false,
                                    vars: [("_".to_owned(), value.clone())].into_iter().collect(),
                                });
                                vm.execute().await;
                                if pop_value!(vm) == Value::Bool(true) {
                                    sta.item(value).unwrap();
                                }

                                ai += ns;
                            }
                        },
                    }
                }
            }

            buf.drain(..consumed);
            consumed = 0;
        }

        match s {
            S::V | S::H => (), // TODO: handle incomplete data
            S::PT(_, stt) => stt.end()?,
            S::PA(sta) => sta.end()?,
        }

        Ok(())
    }
}

use std::sync::Arc;
use std::thread::{spawn, JoinHandle};
use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Sender, Receiver};
use v8::{self, inspector::StringView};
use serde_json::Value;
use tracing::{debug, error};
use super::adjunct::Adjunct;
use super::channel::{oneshot, Rx};
use super::context::{Context, Call, Export, Find};
use super::inspect::Inspector;
use super::promise::{Promise, Promises};

pub struct Machine {
    module: String,
    extra:  Vec<Box<dyn Adjunct>>,
}

#[derive(Clone)]
pub struct Handle {
    sender: Sender<Command>,
}

pub struct Guard {
    handle: Handle,
    thread: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct Function {
    export: Export,
    handle: Handle,
}

pub trait Args {
    fn args(self) -> Vec<Value>;
}

struct Thread {
    module:   String,
    extra:    Vec<Box<dyn Adjunct>>,
    receiver: Receiver<Command>,
    handle:   Handle,
}

pub enum Command {
    Call(Call),
    Find(Find),
    Done(Promise),
    Tick,
    Stop,
}

impl Machine {
    pub fn new(module: String) -> Self {
        let extra = Vec::new();
        Self { module, extra }
    }

    pub fn extend<T: Adjunct>(&mut self, adjunct: Box<T>) {
        self.extra.push(adjunct);
    }

    pub fn exec(self) -> (Handle, Guard) {
        let (sender, receiver) = unbounded();

        let handle = Handle { sender };
        let thread = Thread {
            module:   self.module,
            extra:    self.extra,
            receiver: receiver,
            handle:   handle.clone(),
        };

        let thread = spawn(move || {
            match thread.exec() {
                Ok(()) => debug!("machine finished"),
                Err(e) => error!("machine failed: {e:?}"),
            }
        });

        let guard  = Guard {
            handle: handle.clone(),
            thread: Some(thread),
        };

        (handle, guard)
    }
}

impl Handle {
    pub fn find(&self, export: &str) -> Result<Function> {
        let (sender, receiver) = unbounded();
        let export = Arc::new(export.to_owned());
        let handle = self.clone();

        self.send(Command::Find(Find {
            export: export.clone(),
            sender: sender,
        }))?;

        let export = receiver.recv()??;
        Ok(Function { export, handle })
    }

    pub fn done(&self, promise: Promise) -> Result<()> {
        self.send(Command::Done(promise))
    }

    pub fn tick(&self) -> Result<()> {
        self.send(Command::Tick)
    }

    fn send(&self, cmd: Command) -> Result<()> {
        match self.sender.send(cmd) {
            Ok(()) => Ok(()),
            Err(_) => Err(anyhow!("machine terminated")),
        }
    }
}

impl Function {
    pub fn call<A: Args>(&self, args: A) -> Result<Rx> {
        let (tx, rx) = oneshot();
        let export = self.export.clone();
        self.handle.send(Command::Call(Call {
            export: export,
            args:   args.args(),
            sender: tx,
        }))?;
        Ok(rx)
    }
}

impl Thread {
    fn exec(self) -> Result<()> {
        let Self { module, extra, receiver, handle } = self;

        let mut promises  = Promises::new(handle);

        let mut isolate   = v8::Isolate::new(v8::CreateParams::default());
        let mut inspector = Inspector::new();
        let mut inspector = inspector.create(&mut isolate);

        let scope  = &mut v8::HandleScope::new(&mut isolate);
        let global = v8::ObjectTemplate::new(scope);
        global.set_internal_field_count(1);

        for adjunct in &extra {
            adjunct.install(scope, &global);
        }

        let context   = v8::Context::new_from_template(scope, global);
        let mut scope = v8::ContextScope::new(scope, context);

        let promises = &mut promises as *mut Promises;
        let promises = v8::External::new(&mut scope, promises as _);

        let global = context.global(&mut scope);
        global.set_internal_field(0, promises.into());

        let name = StringView::from(b"".as_slice());
        inspector.context_created(context, 1, name);

        let mut context = Context::new(scope, &module)?;

        loop {
            match receiver.recv() {
                Ok(Command::Call(call))    => context.call(call)?,
                Ok(Command::Find(find))    => context.find(find)?,
                Ok(Command::Done(promise)) => context.done(promise)?,
                Ok(Command::Tick)          => (),
                Ok(Command::Stop) | Err(_) => break,
            }
            context.tick();
        }

        Ok(())
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = self.handle.send(Command::Stop);
            match handle.join() {
                Ok(()) => (),
                Err(e) => error!("join error: {e:?}"),
            }
        }
    }
}

impl Args for () {
    fn args(self) -> Vec<Value> {
        Vec::new()
    }
}

impl Args for Value {
    fn args(self) -> Vec<Value> {
        vec![self]
    }
}

impl Args for Vec<Value> {
    fn args(self) -> Vec<Value> {
        self
    }
}

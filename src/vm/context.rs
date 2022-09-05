use std::fmt::Write;
use std::sync::Arc;
use anyhow::{anyhow, Error, Result};
use crossbeam_channel::Sender;
use serde_json::Value;
use v8::{self, ContextScope, Function, HandleScope, Local, Weak};
use v8::script_compiler::{compile_module, Source};
use super::channel::Tx;
use super::promise::{Promise, Promises};

pub struct Context<'i, 's> {
    pub context: Local<'s, v8::Context>,
    pub scope:   ContextScope<'i, HandleScope<'s>>,
    pub exports: Local<'s, v8::Object>,
}

pub struct Call {
    pub export: Export,
    pub args:   Vec<Value>,
    pub sender: Tx,
}

pub struct Find {
    pub export: Arc<String>,
    pub sender: Sender<Result<Export>>,
}

#[derive(Clone)]
pub struct Export {
    weak: Arc<Weak<Function>>,
}

unsafe impl Send for Export {}
unsafe impl Sync for Export {}

impl<'i, 's> Context<'i, 's> {
    pub fn new(mut scope: ContextScope<'i, HandleScope<'s>>, module: &str) -> Result<Self> {
        let context = scope.get_current_context();

        let exports = {
            let scope = &mut v8::TryCatch::new(&mut scope);

            let module = match compile(scope, module) {
                Some(module) => module,
                None         => return Err(failure(scope)),
            };

            let object = module.get_module_namespace();
            object.to_object(scope).unwrap()
        };

        Ok(Self {
            context: context,
            scope:   scope,
            exports: exports,
        })
    }

    pub fn call(&mut self, Call { export, args, sender }: Call) -> Result<()> {
        let scope = &mut v8::HandleScope::new(&mut self.scope);
        let scope = &mut v8::TryCatch::new(scope);

        let func = match export.weak.to_local(scope) {
            Some(func) => func,
            None       => return Err(anyhow!("export gone")),
        };

        let this = self.context.global(scope).into();
        let args = args.into_iter().map(|arg| {
            Ok(serde_v8::to_v8(scope, arg)?)
        }).collect::<Result<Vec<_>>>()?;

        let result = match func.call(scope, this, &args) {
            Some(result) => result,
            None         => v8::undefined(scope).into(),
        };

        if !result.is_promise() {
            let value = match scope.exception() {
                None    => Ok(serde_v8::from_v8(scope, result)?),
                Some(e) => Err(cause(scope, e)),
            };
            sender.send(value);
            return Ok(());
        }

        let tx = Box::leak(Box::new(sender)) as *mut Tx;
        let tx = v8::External::new(scope, tx as _).into();

        let resolved = v8::Function::builder(resolved).data(tx).build(scope).unwrap();
        let rejected = v8::Function::builder(rejected).data(tx).build(scope).unwrap();

        let promise = v8::Local::<v8::Promise>::try_from(result)?;
        promise.then2(scope, resolved, rejected).unwrap();

        Ok(())
    }

    pub fn find(&mut self, Find { export, sender }: Find) -> Result<()> {
        let scope = &mut v8::HandleScope::new(&mut self.scope);

        let name = v8::String::new(scope, &export).unwrap();
        let func = match self.exports.get(scope, name.into()) {
            Some(func) => func,
            None       => v8::undefined(scope).into(),
        };

        let result = match v8::Local::<v8::Function>::try_from(func) {
            Ok(f)  => Ok(Export::new(Weak::new(scope, f))),
            Err(_) => Err(anyhow!("{export} is not a function")),
        };

        sender.send(result).or(Ok(()))
    }

    pub fn done(&mut self, promise: Promise) -> Result<()> {
        let scope  = &mut v8::HandleScope::new(&mut self.scope);
        let global = self.context.global(scope);

        let promises = global.get_internal_field(scope, 0).unwrap();
        Promises::settle(promises, scope, promise).unwrap();

        Ok(())
    }

    pub fn tick(&mut self) {
        let platform = &v8::V8::get_current_platform();
        let scope    = &mut self.scope;
        v8::Platform::pump_message_loop(platform, scope, false);
        scope.perform_microtask_checkpoint();
    }
}

impl Export {
    fn new(weak: Weak<Function>) -> Self {
        Self { weak: Arc::new(weak) }
    }
}

fn resolved(
  scope:   &mut v8::HandleScope,
  args:    v8::FunctionCallbackArguments,
  _result: v8::ReturnValue,
) {
    let scope = &mut v8::HandleScope::new(scope);

    let data  = args.data().unwrap();
    let data  = v8::Local::<v8::External>::try_from(data).unwrap();
    let tx    = unsafe { Box::from_raw(data.value() as *mut Tx) };

    let value = serde_v8::from_v8(scope, args.get(0)).unwrap();

    tx.send(Ok(value));
}

fn rejected(
  scope:   &mut v8::HandleScope,
  args:    v8::FunctionCallbackArguments,
  _result: v8::ReturnValue,
) {
    let scope = &mut v8::HandleScope::new(scope);

    let data  = args.data().unwrap();
    let data  = v8::Local::<v8::External>::try_from(data).unwrap();
    let tx    = unsafe { Box::from_raw(data.value() as *mut Tx) };

    let value = cause(scope, args.get(0));

    tx.send(Err(value));
}

fn cause<'a>(scope: &'a mut HandleScope, mut value: Local<'a, v8::Value>) -> Value {
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
        let context = scope.get_current_context();
        let global  = context.global(scope);

        let name  = v8::String::new(scope, "Error").unwrap();
        let error = global.get(scope, name.into()).unwrap();

        if let Ok(error) = v8::Local::<v8::Object>::try_from(error) {
            if let Some(true) = object.instance_of(scope, error) {
                let name = v8::String::new(scope, "toString").unwrap();
                let func = object.get(scope, name.into()).unwrap();
                if let Ok(func) = v8::Local::<v8::Function>::try_from(func) {
                    value = func.call(scope, object.into(), &[]).unwrap();
                }
            }
        }
    }
    serde_v8::from_v8(scope, value).unwrap()
}

fn compile<'i, 's>(
    scope: &mut v8::TryCatch<'i, v8::HandleScope<'s>>,
    code:  &str
) -> Option<v8::Local<'s, v8::Module>> {
    let code   = v8::String::new(scope, code)?;
    let name   = v8::String::new(scope, "<script>")?;
    let srcmap = v8::undefined(scope);
    let origin = v8::ScriptOrigin::new(
        scope,
        name.into(),
        0,
        0,
        false,
        0,
        srcmap.into(),
        false,
        false,
        true,
    );

    let source = Source::new(code, Some(&origin));

    let module = compile_module(scope, source)?;
    module.instantiate_module(scope, |_, _, _, _| None)?;
    module.evaluate(scope)?;

    Some(module)
}

pub fn failure(scope: &mut v8::TryCatch<v8::HandleScope>) -> Error {
    anyhow!(message(scope).or_else(|| {
        scope.exception().map(|s| s.to_rust_string_lossy(scope))
    }).unwrap_or_else(|| {
        "no exception or message".to_owned()
    }))
}

fn message(scope: &mut v8::TryCatch<v8::HandleScope>) -> Option<String> {
    let msg    = scope.message()?;
    let text   = msg.get(scope).to_rust_string_lossy(scope);
    let script = msg.get_script_resource_name(scope)?.to_rust_string_lossy(scope);
    let source = msg.get_source_line(scope)?.to_rust_string_lossy(scope);
    let line   = msg.get_line_number(scope)?;
    let column = msg.get_start_column();
    let length = msg.get_end_column().saturating_sub(column);

    let mut e = String::new();
    writeln!(&mut e, "{}", text).unwrap();
    writeln!(&mut e, "{:>4}--> {}:{}:{}", "", script, line, column).unwrap();
    writeln!(&mut e, "{:>4} |   ", "").unwrap();
    writeln!(&mut e, "{:>4} | {}", line, source).unwrap();
    writeln!(&mut e, "{:>4} | {:>3$}{:^>4$}", "", "", "", column, length).unwrap();

    Some(e)
}

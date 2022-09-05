use anyhow::Result;
use serde_json::Value;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, registry};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use v8::{new_default_platform, V8};
use v8vm::{Machine, vm::{Adjunct, Promises}};

const MODULE: &str = r#"
export default function(arg) {
    return adjunct(arg);
}
"#;

fn main() -> Result<()> {
    let mut filter = EnvFilter::from_default_env();
    filter = filter.add_directive(LevelFilter::WARN.into());
    let print = fmt::layer().compact();
    registry().with(filter).with(print).init();

    let platform = new_default_platform(0, false).make_shared();
    V8::initialize_platform(platform);
    V8::initialize();

    let mut machine = Machine::new(MODULE.to_owned());
    machine.extend(Box::new(Extension));

    let (handle, _guard) = machine.exec();
    let function = handle.find("default")?;

    let arg = Value::from("A");
    let ret = function.call(arg.clone())?.recv()?;

    println!("default({}) -> {}", arg, ret);

    Ok(())
}

struct Extension;

impl Adjunct for Extension {
    fn install(&self, scope: &mut v8::HandleScope<()>, global: &v8::ObjectTemplate) {
        let name  = v8::String::new(scope, "adjunct").unwrap();
        let value = v8::FunctionTemplate::new(scope, adjunct);
        global.set(name.into(), value.into());
    }
}

fn adjunct(
  scope:      &mut v8::HandleScope,
  args:       v8::FunctionCallbackArguments,
  mut result: v8::ReturnValue,
) {
    let scope = &mut v8::HandleScope::new(scope);

    let global   = args.this();
    let promises = global.get_internal_field(scope, 0).unwrap();

    let resolver = v8::PromiseResolver::new(scope).unwrap();
    let promise  = resolver.get_promise(scope);

    let resolver = v8::Global::new(scope, resolver);
    let resolver = Promises::insert(promises, resolver).unwrap();

    let arg: Value = serde_v8::from_v8(scope, args.get(0)).unwrap();

    std::thread::spawn(move || {
        resolver.resolve(Box::new(arg)).unwrap()
    });

    result.set(promise.into());
}

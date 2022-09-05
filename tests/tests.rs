use std::collections::HashMap;
use std::path::Path;
use std::fs::read_to_string;
use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use tokio::runtime::Runtime;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, registry};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use v8::{new_default_platform, V8};
use v8vm::{Machine, ex::Fetch};
mod common;

#[derive(Debug, Deserialize)]
#[serde(default)]
struct Test {
    module: String,
    invoke: Invoke,
    expect: Result<Value, String>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct Invoke {
    name: String,
    args: Vec<Value>,
}

fn execute(Test { module, invoke, .. }: &Test) -> Result<Value> {
    let runtime = Runtime::new()?;
    let handle  = runtime.handle().clone();

    let client = common::fetch::HttpClient::new(handle);
    let fetch  = Fetch::new(client);

    let mut machine = Machine::new(module.clone());
    machine.extend(fetch);

    let (handle, _guard) = machine.exec();
    let function = handle.find(&invoke.name)?;

    function.call(invoke.args.clone())?.recv()
}

#[test]
fn test() -> Result<()> {
    let mut filter = EnvFilter::from_default_env();
    filter = filter.add_directive(LevelFilter::WARN.into());
    let print = fmt::layer().compact();
    registry().with(filter).with(print).init();

    let platform = new_default_platform(0, false).make_shared();
    V8::initialize_platform(platform);
    V8::initialize();

    let path = Path::new(env!("CARGO_MANIFEST_DIR"));
    let file = path.join("tests/tests.yml");
    let data = read_to_string(file)?;

    let tests = serde_yaml::from_str::<HashMap<String, Test>>(&data)?;

    for (name, test) in tests {
        println!("  test: {name}");
        let result = execute(&test);
        let result = result.map_err(|e| format!("{e:?}"));
        assert_eq!(result, test.expect);
    }

    Ok(())
}

impl Default for Test {
    fn default() -> Self {
        Self {
            module: "".to_owned(),
            expect: Ok(().into()),
            invoke: Invoke::default(),
        }
    }
}

impl Default for Invoke {
    fn default() -> Self {
        Self {
            name: "default".to_owned(),
            args: Vec::new(),
        }
    }
}

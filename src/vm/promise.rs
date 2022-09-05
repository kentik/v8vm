use std::collections::HashMap;
use anyhow::{anyhow, Error, Result};
use v8::{self, Global, HandleScope, Local, PromiseResolver, Value};
use super::machine::Handle;

pub struct Promises {
    counter: u64,
    handle:  Handle,
    pending: HashMap<u64, Global<PromiseResolver>>,
}

pub enum Promise {
    Success(u64, Box<dyn Resolved>),
    Failure(u64, Box<dyn Resolved>),
}

pub struct Resolver {
    id: u64,
    tx: Handle,
}

pub trait Resolved: Send + 'static {
    fn value<'s>(self: Box<Self>, scope: &mut HandleScope<'s>) -> Result<Local<'s, Value>>;
}

impl Promises {
    pub fn new(handle: Handle) -> Self {
        Self {
            counter: 0,
            handle:  handle,
            pending: HashMap::new(),
        }
    }

    pub fn insert(local: Local<v8::Value>, resolver: Global<PromiseResolver>) -> Result<Resolver> {
        let promises = Self::get(local)?;

        let id = promises.counter;
        let tx = promises.handle.clone();

        promises.counter += 1;
        promises.pending.insert(id, resolver);

        Ok(Resolver { id, tx })
    }

    pub fn settle(local: Local<Value>, scope: &mut HandleScope, promise: Promise) -> Result<()> {
        enum Value<'s> {
            Success(Local<'s, v8::Value>),
            Failure(Local<'s, v8::Value>),
        }

        let (id, value) = match promise {
            Promise::Success(id, v) => (id, Value::Success(v.value(scope)?)),
            Promise::Failure(id, v) => (id, Value::Failure(v.value(scope)?)),
        };

        if let Some(resolver) = Self::get(local)?.pending.remove(&id) {
            let resolver = Local::new(scope, resolver);
            match value {
                Value::Success(v) => resolver.resolve(scope, v),
                Value::Failure(v) => resolver.reject(scope, v),
            };
        }

        Ok(())
    }

    fn get(local: Local<v8::Value>) -> Result<&mut Promises> {
        let promises = Local::<v8::External>::try_from(local)?;
        let promises = promises.value() as *mut Self;
        Ok(unsafe { &mut *promises })
    }
}

impl Resolver {
    pub fn resolve(self, value: Box<dyn Resolved>) -> Result<()> {
        match self.tx.done(Promise::Success(self.id, value)) {
            Ok(()) => Ok(()),
            Err(_) => Err(anyhow!("channel closed")),
        }
    }

    pub fn reject(self, value: Box<dyn Resolved>) -> Result<()> {
        match self.tx.done(Promise::Failure(self.id, value)) {
            Ok(()) => Ok(()),
            Err(_) => Err(anyhow!("channel closed")),
        }
    }
}

impl Resolved for Error {
    fn value<'s>(self: Box<Self>, scope: &mut HandleScope<'s>) -> Result<Local<'s, Value>> {
        let context = scope.get_current_context();
        let global  = context.global(scope);

        let cause = format!("{self:?}");
        let cause = v8::String::new(scope, &cause).unwrap();

        let name  = v8::String::new(scope, "Error").unwrap();
        let ctor  = global.get(scope, name.into()).unwrap();
        let ctor  = v8::Local::<v8::Function>::try_from(ctor)?;
        let args  = &[cause.into()];

        Ok(ctor.new_instance(scope, args).unwrap().into())
    }
}

impl Resolved for serde_json::Value {
    fn value<'s>(self: Box<Self>, scope: &mut HandleScope<'s>) -> Result<Local<'s, Value>> {
        Ok(serde_v8::to_v8(scope, self)?)
    }
}

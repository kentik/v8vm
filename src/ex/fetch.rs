use anyhow::Result;
use http::StatusCode;
use v8::{self, Global, HandleScope, Local, ObjectTemplate, Value};
use crate::vm::{Adjunct, Promises, Resolved, Resolver};

pub struct Fetch<C> {
    client: C,
}

pub struct Request {
    pub method: String,
    pub url:    String,
}

pub struct Response {
    pub status: StatusCode,
    pub body:   String,
}

pub trait Client: Send + 'static {
    fn fetch(&self, request: Request, resolver: Resolver);
}

impl<C: Client> Fetch<C> {
    pub fn new(client: C) -> Box<Self> {
        Box::new(Self { client })
    }

    fn fetch(&self, request: Request, resolver: Resolver) {
        self.client.fetch(request, resolver);
    }
}

impl<C: Client> Adjunct for Fetch<C> {
    fn install(&self, scope: &mut HandleScope<()>, global: &ObjectTemplate) {
        let data = self as *const Self;
        let data = v8::External::new(scope, data as _).into();

        let name  = v8::String::new(scope, "fetch").unwrap();
        let value = v8::FunctionTemplate::builder(fetch::<C>).data(data).build(scope);
        global.set(name.into(), value.into());

        let func = v8::FunctionTemplate::new(scope, response);
        let name = v8::String::new(scope, "Response").unwrap();
        func.set_class_name(name);
        global.set(name.into(), func.into());

        let prototype = func.prototype_template(scope);

        let name = v8::String::new(scope, "status").unwrap();
        prototype.set_accessor(name.into(), status);

        let name  = v8::String::new(scope, "json").unwrap();
        let value = v8::FunctionTemplate::new(scope, json);
        prototype.set(name.into(), value.into());

        let name  = v8::String::new(scope, "text").unwrap();
        let value = v8::FunctionTemplate::new(scope, text);
        prototype.set(name.into(), value.into());

        let instance = func.instance_template(scope);
        instance.set_internal_field_count(2);
    }
}

impl Resolved for Response {
    fn value<'s>(self: Box<Self>, scope: &mut HandleScope<'s>) -> Result<Local<'s, Value>> {
        let context = scope.get_current_context();
        let global  = context.global(scope);

        let body    = v8::String::new(scope, &self.body).unwrap();
        let options = v8::Object::new(scope);

        let status = v8::Number::new(scope, self.status.as_u16().into());
        let name   = v8::String::new(scope, "status").unwrap();
        options.set(scope, name.into(), status.into());

        let name = v8::String::new(scope, "Response").unwrap();
        let ctor = global.get(scope, name.into()).unwrap();
        let ctor = v8::Local::<v8::Function>::try_from(ctor)?;
        let args = &[body.into(), options.into()];

        Ok(ctor.new_instance(scope, args).unwrap().into())
    }
}

fn response(
  scope:      &mut v8::HandleScope,
  args:       v8::FunctionCallbackArguments,
  mut result: v8::ReturnValue,
) {
    let scope  = &mut v8::HandleScope::new(scope);
    let object = args.this();

    let body = match Local::<v8::String>::try_from(args.get(0)) {
        Ok(string) => string,
        Err(_)     => v8::String::new(scope, "").unwrap(),
    };

    let options = match Local::<v8::Object>::try_from(args.get(1)) {
        Ok(object) => object,
        Err(_)     => v8::Object::new(scope),
    };

    let name = v8::String::new(scope, "status").unwrap().into();
    let status = match options.get(scope, name).map(Local::<v8::Number>::try_from) {
        Some(Ok(number))    => number,
        Some(Err(_)) | None => v8::Number::new(scope, 200.0),
    };

    object.set_internal_field(0, status.into());
    object.set_internal_field(1, body.into());

    result.set(object.into());
}

fn status(
    scope:      &mut v8::HandleScope,
    _key:       Local<v8::Name>,
    args:       v8::PropertyCallbackArguments,
    mut result: v8::ReturnValue,
) {

    let this   = args.this();
    let status = this.get_internal_field(scope, 0).unwrap();
    result.set(status);
}

fn json(
    scope:      &mut v8::HandleScope,
    args:       v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {

    let this = args.this();

    let body = this.get_internal_field(scope, 1).unwrap();
    let body = v8::Local::<v8::String>::try_from(body).unwrap();

    let json = match v8::json::parse(scope, body) {
        Some(json) => json,
        None       => v8::undefined(scope).into(),
    };

    let resolver = v8::PromiseResolver::new(scope).unwrap();
    resolver.resolve(scope, json).unwrap();

    result.set(resolver.get_promise(scope).into());
}

fn text(
    scope:      &mut v8::HandleScope,
    args:       v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {

    let this = args.this();

    let body = this.get_internal_field(scope, 1).unwrap();
    let body = v8::Local::<v8::String>::try_from(body).unwrap();

    let resolver = v8::PromiseResolver::new(scope).unwrap();
    resolver.resolve(scope, body.into()).unwrap();

    result.set(resolver.get_promise(scope).into());
}

fn fetch<C: Client>(
  scope:      &mut v8::HandleScope,
  args:       v8::FunctionCallbackArguments,
  mut result: v8::ReturnValue,
) {
    let scope = &mut v8::HandleScope::new(scope);

    let global   = args.this();
    let promises = global.get_internal_field(scope, 0).unwrap();

    let resolver = v8::PromiseResolver::new(scope).unwrap();
    let promise  = resolver.get_promise(scope);

    let resolver = Global::new(scope, resolver);
    let resolver = Promises::insert(promises, resolver).unwrap();

    let url = match Local::<v8::String>::try_from(args.get(0)) {
        Ok(string) => string.to_rust_string_lossy(scope),
        Err(_)     => "".to_owned(),
    };

    let options = match Local::<v8::Object>::try_from(args.get(1)) {
        Ok(object) => object,
        Err(_)     => v8::Object::new(scope),
    };

    let name   = v8::String::new(scope, "method").unwrap().into();
    let method = match options.get(scope, name).map(Local::<v8::String>::try_from) {
        Some(Ok(method))    => method.to_rust_string_lossy(scope),
        Some(Err(_)) | None => "GET".to_owned(),
    };

    let request = Request {
        method: method,
        url:    url,
    };

    let data  = args.data().unwrap();
    let data  = v8::Local::<v8::External>::try_from(data).unwrap();
    let fetch = data.value() as *const Fetch<C>;
    let fetch = unsafe { &*fetch };

    fetch.fetch(request, resolver);

    result.set(promise.into());
}

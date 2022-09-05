use v8::{HandleScope, ObjectTemplate};

pub trait Adjunct: Send + 'static {
    fn install(&self, scope: &mut HandleScope<()>, global: &ObjectTemplate);
}

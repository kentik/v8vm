use v8::{Isolate, UniqueRef};
use v8::inspector::*;
use tracing::{event, Level};

pub struct Inspector {
    base: V8InspectorClientBase,
}

impl Inspector {
    pub fn new() -> Self {
        Self {
            base: V8InspectorClientBase::new::<Self>(),
        }
    }

    pub fn create(&mut self, isolate: &mut Isolate) -> UniqueRef<V8Inspector> {
        V8Inspector::create(isolate, self)
    }
}

impl V8InspectorClientImpl for Inspector {
    fn base(&self) -> &V8InspectorClientBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut V8InspectorClientBase {
        &mut self.base
    }

    fn console_api_message(
        &mut self,
        _context_group_id: i32,
        level:            i32,
        message:           &StringView,
        _url:              &StringView,
        _line_number:      u32,
        _column_number:    u32,
        _stack_trace:      &mut V8StackTrace,
    ) {
        match level {
            INFO  => event!(target: "<script>", Level::INFO,  "{}", message),
            DEBUG => event!(target: "<script>", Level::DEBUG, "{}", message),
            TRACE => event!(target: "<script>", Level::TRACE, "{}", message),
            ERROR => event!(target: "<script>", Level::ERROR, "{}", message),
            WARN  => event!(target: "<script>", Level::WARN,  "{}", message),
            _     => event!(target: "<script>", Level::INFO,  "{}", message),
        }
    }
}

const INFO:  i32 = 1 << 0;
const DEBUG: i32 = 1 << 1;
const TRACE: i32 = 1 << 2;
const ERROR: i32 = 1 << 3;
const WARN:  i32 = 1 << 4;

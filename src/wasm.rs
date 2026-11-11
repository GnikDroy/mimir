//! C ABI for WebAssembly hosts.

use std::alloc::{alloc, dealloc, Layout};
use std::sync::Mutex;

use crate::uci::adapter::UCIAdapter;
use crate::uci::parser::UCICommand;

/// Everything the engine has printed since the last drain.
static OUTPUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// The engine, built on first use so the host need not initialise it.
static ADAPTER: Mutex<Option<UCIAdapter>> = Mutex::new(None);

/// Writer handed to the adapter, collecting output for the host to
/// collect rather than sending it to a stdout (which does not exist).
struct OutputBuffer;

impl std::io::Write for OutputBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut out) = OUTPUT.lock() {
            out.extend_from_slice(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn mimir_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    match Layout::array::<u8>(len) {
        // SAFETY: `len` is nonzero, so the layout has nonzero size.
        Ok(layout) => unsafe { alloc(layout) },
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn mimir_dealloc(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    if let Ok(layout) = Layout::array::<u8>(len) {
        dealloc(ptr, layout);
    }
}

#[no_mangle]
pub unsafe extern "C" fn mimir_command(ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let bytes = std::slice::from_raw_parts(ptr, len);
    let Ok(command) = std::str::from_utf8(bytes) else {
        return;
    };

    let Ok(mut slot) = ADAPTER.lock() else {
        return;
    };
    let adapter = slot.get_or_insert_with(|| UCIAdapter::with_writer(Box::new(OutputBuffer)));

    for line in command.lines() {
        if !line.trim().is_empty() {
            adapter.handle_command(UCICommand::parse(line.trim()));
        }
    }
}

#[no_mangle]
pub extern "C" fn mimir_output_len() -> usize {
    OUTPUT.lock().map(|out| out.len()).unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn mimir_output_copy(dest: *mut u8) {
    if dest.is_null() {
        return;
    }
    if let Ok(mut out) = OUTPUT.lock() {
        std::ptr::copy_nonoverlapping(out.as_ptr(), dest, out.len());
        out.clear();
    }
}

#[no_mangle]
pub extern "C" fn mimir_reset() {
    if let Ok(mut slot) = ADAPTER.lock() {
        *slot = None;
    }
    if let Ok(mut out) = OUTPUT.lock() {
        out.clear();
    }
}

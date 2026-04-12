#![allow(non_camel_case_types)]

use portable_pty::{
    native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize,
};
use std::ffi::{c_char, c_void, CStr, CString};
use std::io::{Read, Write};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use wezterm_escape_parser::osc::OperatingSystemCommand;
use wezterm_escape_parser::parser::Parser;
use wezterm_escape_parser::{Action, ControlCode};

const WEZTERM_KIT_ABI_VERSION: u32 = 1;

pub const WEZTERM_KIT_STATUS_OK: i32 = 0;
pub const WEZTERM_KIT_STATUS_INVALID_ARGUMENT: i32 = 1;
pub const WEZTERM_KIT_STATUS_ALREADY_RUNNING: i32 = 2;
pub const WEZTERM_KIT_STATUS_NOT_RUNNING: i32 = 3;
pub const WEZTERM_KIT_STATUS_INTERNAL_ERROR: i32 = 255;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct wezterm_kit_size_t {
    pub rows: u16,
    pub cols: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct wezterm_kit_env_var_t {
    pub key: *const c_char,
    pub value: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct wezterm_kit_spawn_config_t {
    pub program: *const c_char,
    pub argv: *const *const c_char,
    pub argc: usize,
    pub env: *const wezterm_kit_env_var_t,
    pub env_count: usize,
    pub cwd: *const c_char,
    pub size: wezterm_kit_size_t,
    pub clear_environment: bool,
    pub controlling_tty: bool,
}

impl Default for wezterm_kit_spawn_config_t {
    fn default() -> Self {
        Self {
            program: ptr::null(),
            argv: ptr::null(),
            argc: 0,
            env: ptr::null(),
            env_count: 0,
            cwd: ptr::null(),
            size: wezterm_kit_size_t {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
            clear_environment: false,
            controlling_tty: true,
        }
    }
}

pub type wezterm_kit_on_data_cb = extern "C" fn(*mut c_void, *const u8, usize);
pub type wezterm_kit_on_string_cb = extern "C" fn(*mut c_void, *const c_char);
pub type wezterm_kit_on_bell_cb = extern "C" fn(*mut c_void);
pub type wezterm_kit_on_exit_cb = extern "C" fn(*mut c_void, i32);
pub type wezterm_kit_on_log_cb = extern "C" fn(*mut c_void, i32, *const c_char);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct wezterm_kit_callbacks_t {
    pub user_data: *mut c_void,
    pub on_data: Option<wezterm_kit_on_data_cb>,
    pub on_title: Option<wezterm_kit_on_string_cb>,
    pub on_working_directory: Option<wezterm_kit_on_string_cb>,
    pub on_bell: Option<wezterm_kit_on_bell_cb>,
    pub on_exit: Option<wezterm_kit_on_exit_cb>,
    pub on_log: Option<wezterm_kit_on_log_cb>,
}

// The embedding application owns the pointed-to user data and decides whether it is thread-safe.
unsafe impl Send for wezterm_kit_callbacks_t {}
unsafe impl Sync for wezterm_kit_callbacks_t {}

#[repr(C)]
pub struct wezterm_kit_session_t {
    _private: [u8; 0],
}

struct SessionCore {
    callbacks: Mutex<wezterm_kit_callbacks_t>,
    killer: Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    read_thread: Mutex<Option<JoinHandle<()>>>,
    wait_thread: Mutex<Option<JoinHandle<()>>>,
    running: AtomicBool,
    last_title: Mutex<Option<String>>,
    last_working_directory: Mutex<Option<String>>,
}

impl SessionCore {
    fn new(callbacks: wezterm_kit_callbacks_t) -> Self {
        Self {
            callbacks: Mutex::new(callbacks),
            killer: Mutex::new(None),
            master: Mutex::new(None),
            writer: Mutex::new(None),
            read_thread: Mutex::new(None),
            wait_thread: Mutex::new(None),
            running: AtomicBool::new(false),
            last_title: Mutex::new(None),
            last_working_directory: Mutex::new(None),
        }
    }

    fn set_callbacks(&self, callbacks: wezterm_kit_callbacks_t) {
        if let Ok(mut slot) = self.callbacks.lock() {
            *slot = callbacks;
        }
    }

    fn emit_data(&self, bytes: &[u8]) {
        let callbacks = self.callbacks.lock().expect("callbacks mutex poisoned");
        if let Some(cb) = callbacks.on_data {
            cb(callbacks.user_data, bytes.as_ptr(), bytes.len());
        }
    }

    fn emit_bell(&self) {
        let callbacks = self.callbacks.lock().expect("callbacks mutex poisoned");
        if let Some(cb) = callbacks.on_bell {
            cb(callbacks.user_data);
        }
    }

    fn emit_exit(&self, exit_code: i32) {
        let callbacks = self.callbacks.lock().expect("callbacks mutex poisoned");
        if let Some(cb) = callbacks.on_exit {
            cb(callbacks.user_data, exit_code);
        }
    }

    fn emit_log(&self, level: i32, message: impl AsRef<str>) {
        let callbacks = self.callbacks.lock().expect("callbacks mutex poisoned");
        let Some(cb) = callbacks.on_log else {
            return;
        };
        if let Some(message) = cstring_from_lossy(message.as_ref()) {
            cb(callbacks.user_data, level, message.as_ptr());
        }
    }

    fn emit_title(&self, title: String) {
        if !Self::replace_if_changed(&self.last_title, &title) {
            return;
        }
        let callbacks = self.callbacks.lock().expect("callbacks mutex poisoned");
        let Some(cb) = callbacks.on_title else {
            return;
        };
        if let Some(title) = cstring_from_lossy(&title) {
            cb(callbacks.user_data, title.as_ptr());
        }
    }

    fn emit_working_directory(&self, cwd: String) {
        if !Self::replace_if_changed(&self.last_working_directory, &cwd) {
            return;
        }
        let callbacks = self.callbacks.lock().expect("callbacks mutex poisoned");
        let Some(cb) = callbacks.on_working_directory else {
            return;
        };
        if let Some(cwd) = cstring_from_lossy(&cwd) {
            cb(callbacks.user_data, cwd.as_ptr());
        }
    }

    fn replace_if_changed(slot: &Mutex<Option<String>>, value: &str) -> bool {
        let mut slot = slot.lock().expect("state mutex poisoned");
        if slot.as_deref() == Some(value) {
            return false;
        }
        *slot = Some(value.to_owned());
        true
    }

    fn clear_runtime_state(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.killer.lock().expect("killer mutex poisoned").take();
        self.master.lock().expect("master mutex poisoned").take();
        self.writer.lock().expect("writer mutex poisoned").take();
    }

    fn shutdown(&self) {
        self.set_callbacks(wezterm_kit_callbacks_t::default());
        self.running.store(false, Ordering::SeqCst);
        if let Some(mut killer) = self.killer.lock().expect("killer mutex poisoned").take() {
            let _ = killer.kill();
        }
        self.master.lock().expect("master mutex poisoned").take();
        self.writer.lock().expect("writer mutex poisoned").take();
    }
}

struct WezTermKitSession {
    core: Arc<SessionCore>,
}

#[no_mangle]
pub extern "C" fn wezterm_kit_abi_version() -> u32 {
    WEZTERM_KIT_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn wezterm_kit_status_string(status: i32) -> *const c_char {
    match status {
        WEZTERM_KIT_STATUS_OK => c"ok".as_ptr(),
        WEZTERM_KIT_STATUS_INVALID_ARGUMENT => c"invalid_argument".as_ptr(),
        WEZTERM_KIT_STATUS_ALREADY_RUNNING => c"already_running".as_ptr(),
        WEZTERM_KIT_STATUS_NOT_RUNNING => c"not_running".as_ptr(),
        _ => c"internal_error".as_ptr(),
    }
}

#[no_mangle]
pub extern "C" fn wezterm_kit_session_new(
    callbacks: wezterm_kit_callbacks_t,
    out_session: *mut *mut wezterm_kit_session_t,
) -> i32 {
    if out_session.is_null() {
        return WEZTERM_KIT_STATUS_INVALID_ARGUMENT;
    }

    let handle = Box::new(WezTermKitSession {
        core: Arc::new(SessionCore::new(callbacks)),
    });

    // SAFETY: out_session is checked for null above and points to caller-owned storage.
    unsafe {
        *out_session = Box::into_raw(handle) as *mut wezterm_kit_session_t;
    }
    WEZTERM_KIT_STATUS_OK
}

#[no_mangle]
pub extern "C" fn wezterm_kit_session_set_callbacks(
    session: *mut wezterm_kit_session_t,
    callbacks: wezterm_kit_callbacks_t,
) -> i32 {
    let Some(handle) = session_ref(session) else {
        return WEZTERM_KIT_STATUS_INVALID_ARGUMENT;
    };
    handle.core.set_callbacks(callbacks);
    WEZTERM_KIT_STATUS_OK
}

#[no_mangle]
pub extern "C" fn wezterm_kit_session_free(session: *mut wezterm_kit_session_t) {
    if session.is_null() {
        return;
    }

    // SAFETY: session was allocated by `wezterm_kit_session_new` and must be consumed exactly once here.
    let handle = unsafe { Box::from_raw(session as *mut WezTermKitSession) };
    handle.core.shutdown();
}

#[no_mangle]
pub extern "C" fn wezterm_kit_session_spawn_local(
    session: *mut wezterm_kit_session_t,
    config: *const wezterm_kit_spawn_config_t,
) -> i32 {
    let Some(handle) = session_ref(session) else {
        return WEZTERM_KIT_STATUS_INVALID_ARGUMENT;
    };
    let Some(config) = config_ref(config) else {
        return WEZTERM_KIT_STATUS_INVALID_ARGUMENT;
    };
    if handle.core.running.load(Ordering::SeqCst) {
        return WEZTERM_KIT_STATUS_ALREADY_RUNNING;
    }

    match spawn_local(handle.core.clone(), config) {
        Ok(()) => WEZTERM_KIT_STATUS_OK,
        Err(err) => {
            handle.core.emit_log(3, format!("spawn_local failed: {err}"));
            WEZTERM_KIT_STATUS_INTERNAL_ERROR
        }
    }
}

#[no_mangle]
pub extern "C" fn wezterm_kit_session_write(
    session: *mut wezterm_kit_session_t,
    data: *const u8,
    len: usize,
) -> i32 {
    let Some(handle) = session_ref(session) else {
        return WEZTERM_KIT_STATUS_INVALID_ARGUMENT;
    };
    if data.is_null() || len == 0 {
        return WEZTERM_KIT_STATUS_INVALID_ARGUMENT;
    }
    if !handle.core.running.load(Ordering::SeqCst) {
        return WEZTERM_KIT_STATUS_NOT_RUNNING;
    }

    // SAFETY: caller provides `len` bytes starting at `data`.
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    let mut writer_guard = handle.core.writer.lock().expect("writer mutex poisoned");
    let Some(writer) = writer_guard.as_mut() else {
        return WEZTERM_KIT_STATUS_NOT_RUNNING;
    };
    if writer.write_all(bytes).is_err() || writer.flush().is_err() {
        return WEZTERM_KIT_STATUS_INTERNAL_ERROR;
    }
    WEZTERM_KIT_STATUS_OK
}

#[no_mangle]
pub extern "C" fn wezterm_kit_session_write_text(
    session: *mut wezterm_kit_session_t,
    text: *const c_char,
) -> i32 {
    let Some(text) = cstr_to_bytes(text) else {
        return WEZTERM_KIT_STATUS_INVALID_ARGUMENT;
    };
    wezterm_kit_session_write(session, text.as_ptr(), text.len())
}

#[no_mangle]
pub extern "C" fn wezterm_kit_session_resize(
    session: *mut wezterm_kit_session_t,
    size: wezterm_kit_size_t,
) -> i32 {
    let Some(handle) = session_ref(session) else {
        return WEZTERM_KIT_STATUS_INVALID_ARGUMENT;
    };
    if !handle.core.running.load(Ordering::SeqCst) {
        return WEZTERM_KIT_STATUS_NOT_RUNNING;
    }

    let pty_size = to_pty_size(size);
    let master = handle.core.master.lock().expect("master mutex poisoned");
    let Some(master) = master.as_ref() else {
        return WEZTERM_KIT_STATUS_NOT_RUNNING;
    };
    if master.resize(pty_size).is_err() {
        return WEZTERM_KIT_STATUS_INTERNAL_ERROR;
    }
    WEZTERM_KIT_STATUS_OK
}

#[no_mangle]
pub extern "C" fn wezterm_kit_session_kill(session: *mut wezterm_kit_session_t) -> i32 {
    let Some(handle) = session_ref(session) else {
        return WEZTERM_KIT_STATUS_INVALID_ARGUMENT;
    };
    let mut killer = handle.core.killer.lock().expect("killer mutex poisoned");
    let Some(killer) = killer.as_mut() else {
        return WEZTERM_KIT_STATUS_NOT_RUNNING;
    };
    if killer.kill().is_err() {
        return WEZTERM_KIT_STATUS_INTERNAL_ERROR;
    }
    WEZTERM_KIT_STATUS_OK
}

fn spawn_local(core: Arc<SessionCore>, config: &wezterm_kit_spawn_config_t) -> anyhow::Result<()> {
    let program = cstr_to_string(config.program).ok_or_else(|| anyhow::anyhow!("program is required"))?;
    let system = native_pty_system();
    let pty_size = to_pty_size(config.size);
    let pair = system.openpty(pty_size)?;

    let mut cmd = CommandBuilder::new(&program);
    let argv = cstr_array_to_strings(config.argv, config.argc)?;
    if !argv.is_empty() {
        cmd.args(argv);
    }
    if config.clear_environment {
        cmd.env_clear();
    }
    for env in env_slice(config.env, config.env_count)? {
        let key = cstr_to_string(env.key).ok_or_else(|| anyhow::anyhow!("env key is null"))?;
        let value = cstr_to_string(env.value).ok_or_else(|| anyhow::anyhow!("env value is null"))?;
        cmd.env(key, value);
    }
    if let Some(cwd) = cstr_to_string(config.cwd) {
        cmd.cwd(cwd);
    }
    cmd.set_controlling_tty(config.controlling_tty);

    let mut child = pair.slave.spawn_command(cmd)?;
    let killer = child.clone_killer();
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;

    {
        let mut killer_slot = core.killer.lock().expect("killer mutex poisoned");
        *killer_slot = Some(killer);
    }
    {
        let mut master_slot = core.master.lock().expect("master mutex poisoned");
        *master_slot = Some(pair.master);
    }
    {
        let mut writer_slot = core.writer.lock().expect("writer mutex poisoned");
        *writer_slot = Some(writer);
    }
    core.running.store(true, Ordering::SeqCst);

    let read_core = core.clone();
    let read_thread = thread::spawn(move || {
        let mut parser = Parser::new();
        let mut buffer = [0u8; 8192];

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read_len) => {
                    let bytes = &buffer[..read_len];
                    read_core.emit_data(bytes);
                    parser.parse(bytes, |action| handle_parsed_action(&read_core, action));
                }
                Err(err) => {
                    read_core.emit_log(3, format!("reader failed: {err}"));
                    break;
                }
            }
        }
    });
    *core.read_thread.lock().expect("read thread mutex poisoned") = Some(read_thread);

    let wait_core = core.clone();
    let wait_thread = thread::spawn(move || {
        let exit_code = match child.wait() {
            Ok(status) => status.exit_code() as i32,
            Err(err) => {
                wait_core.emit_log(3, format!("wait failed: {err}"));
                1
            }
        };
        wait_core.clear_runtime_state();
        wait_core.emit_exit(exit_code);
    });
    *core.wait_thread.lock().expect("wait thread mutex poisoned") = Some(wait_thread);

    Ok(())
}

fn handle_parsed_action(core: &SessionCore, action: Action) {
    match action {
        Action::Control(ControlCode::Bell) => core.emit_bell(),
        Action::OperatingSystemCommand(osc) => match *osc {
            OperatingSystemCommand::SetIconNameAndWindowTitle(title)
            | OperatingSystemCommand::SetWindowTitle(title)
            | OperatingSystemCommand::SetWindowTitleSun(title)
            | OperatingSystemCommand::SetIconName(title)
            | OperatingSystemCommand::SetIconNameSun(title) => core.emit_title(title),
            OperatingSystemCommand::CurrentWorkingDirectory(cwd) => core.emit_working_directory(cwd),
            OperatingSystemCommand::ITermProprietary(proprietary) => {
                let description = format!("{proprietary:?}");
                if let Some(cwd) = description
                    .strip_prefix("CurrentDir(\"")
                    .and_then(|value| value.strip_suffix("\")"))
                {
                    core.emit_working_directory(cwd.to_owned());
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn to_pty_size(size: wezterm_kit_size_t) -> PtySize {
    PtySize {
        rows: if size.rows == 0 { 24 } else { size.rows },
        cols: if size.cols == 0 { 80 } else { size.cols },
        pixel_width: size.pixel_width,
        pixel_height: size.pixel_height,
    }
}

fn env_slice(
    env: *const wezterm_kit_env_var_t,
    env_count: usize,
) -> anyhow::Result<&'static [wezterm_kit_env_var_t]> {
    if env_count == 0 {
        return Ok(&[]);
    }
    if env.is_null() {
        return Err(anyhow::anyhow!("env is null while env_count > 0"));
    }
    // SAFETY: caller provides `env_count` contiguous entries at `env`.
    Ok(unsafe { std::slice::from_raw_parts(env, env_count) })
}

fn cstr_array_to_strings(argv: *const *const c_char, argc: usize) -> anyhow::Result<Vec<String>> {
    if argc == 0 {
        return Ok(Vec::new());
    }
    if argv.is_null() {
        return Err(anyhow::anyhow!("argv is null while argc > 0"));
    }

    // SAFETY: caller provides `argc` contiguous pointers at `argv`.
    let slice = unsafe { std::slice::from_raw_parts(argv, argc) };
    slice
        .iter()
        .map(|value| cstr_to_string(*value).ok_or_else(|| anyhow::anyhow!("argv contains null entry")))
        .collect()
}

fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller promises `ptr` points to a NUL-terminated string.
    let value = unsafe { CStr::from_ptr(ptr) };
    Some(value.to_string_lossy().into_owned())
}

fn cstr_to_bytes(ptr: *const c_char) -> Option<Vec<u8>> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller promises `ptr` points to a NUL-terminated string.
    let value = unsafe { CStr::from_ptr(ptr) };
    Some(value.to_bytes().to_vec())
}

fn cstring_from_lossy(value: &str) -> Option<CString> {
    CString::new(value.replace('\0', " ")).ok()
}

fn session_ref(session: *mut wezterm_kit_session_t) -> Option<&'static WezTermKitSession> {
    if session.is_null() {
        return None;
    }
    // SAFETY: the pointer must come from `wezterm_kit_session_new` and remain valid for the call duration.
    Some(unsafe { &*(session as *mut WezTermKitSession) })
}

fn config_ref(config: *const wezterm_kit_spawn_config_t) -> Option<&'static wezterm_kit_spawn_config_t> {
    if config.is_null() {
        return None;
    }
    // SAFETY: caller keeps the config alive for the duration of the call.
    Some(unsafe { &*config })
}

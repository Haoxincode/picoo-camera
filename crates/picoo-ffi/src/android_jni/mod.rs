//! Android JNI exports implemented directly in Rust.

#![allow(non_snake_case)]

use std::collections::HashMap;
use std::ptr;
use std::sync::{Mutex, OnceLock};

use jni::objects::JString;
use jni::sys::jlong;
use jni::JNIEnv;
use picoo_pairing::DeviceIdentity;

use crate::handles::{BrowserInner, SenderInner, TrustedStoreInner};

mod diagnostics;
mod discovery;
mod pairing;
mod sender;

pub(crate) struct HandleMap<T> {
    next: jlong,
    pub(crate) values: HashMap<jlong, T>,
}

impl<T> Default for HandleMap<T> {
    fn default() -> Self {
        Self {
            next: 1,
            values: HashMap::new(),
        }
    }
}

impl<T> HandleMap<T> {
    pub(crate) fn insert(&mut self, value: T) -> jlong {
        let handle = self.next;
        self.next = self.next.saturating_add(1).max(1);
        self.values.insert(handle, value);
        handle
    }
}

static SENDERS: OnceLock<Mutex<HandleMap<SenderInner>>> = OnceLock::new();
static BROWSERS: OnceLock<Mutex<HandleMap<BrowserInner>>> = OnceLock::new();
static TRUSTED_STORES: OnceLock<Mutex<HandleMap<TrustedStoreInner>>> = OnceLock::new();
static IDENTITIES: OnceLock<Mutex<HandleMap<DeviceIdentity>>> = OnceLock::new();

pub(crate) fn senders() -> &'static Mutex<HandleMap<SenderInner>> {
    SENDERS.get_or_init(|| Mutex::new(HandleMap::default()))
}

pub(crate) fn browsers() -> &'static Mutex<HandleMap<BrowserInner>> {
    BROWSERS.get_or_init(|| Mutex::new(HandleMap::default()))
}

pub(crate) fn trusted_stores() -> &'static Mutex<HandleMap<TrustedStoreInner>> {
    TRUSTED_STORES.get_or_init(|| Mutex::new(HandleMap::default()))
}

pub(crate) fn identities() -> &'static Mutex<HandleMap<DeviceIdentity>> {
    IDENTITIES.get_or_init(|| Mutex::new(HandleMap::default()))
}

pub(crate) fn with_sender<R>(handle: jlong, f: impl FnOnce(&mut SenderInner) -> R) -> Option<R> {
    let mut handles = senders().lock().ok()?;
    Some(f(handles.values.get_mut(&handle)?))
}

pub(crate) fn with_browser<R>(handle: jlong, f: impl FnOnce(&mut BrowserInner) -> R) -> Option<R> {
    let mut handles = browsers().lock().ok()?;
    Some(f(handles.values.get_mut(&handle)?))
}

pub(crate) fn with_trusted_store<R>(
    handle: jlong,
    f: impl FnOnce(&mut TrustedStoreInner) -> R,
) -> Option<R> {
    let mut handles = trusted_stores().lock().ok()?;
    Some(f(handles.values.get_mut(&handle)?))
}

pub(crate) fn with_identity<R>(
    handle: jlong,
    f: impl FnOnce(&mut DeviceIdentity) -> R,
) -> Option<R> {
    let mut handles = identities().lock().ok()?;
    Some(f(handles.values.get_mut(&handle)?))
}

pub(crate) fn java_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Option<String> {
    if value.is_null() {
        return None;
    }
    env.get_string(&value).ok().map(Into::into)
}

pub(crate) fn optional_java_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Option<String> {
    java_string(env, value).filter(|value| !value.is_empty())
}

pub(crate) fn new_java_string(env: &mut JNIEnv<'_>, value: &str) -> jni::sys::jstring {
    env.new_string(value)
        .map(JString::into_raw)
        .unwrap_or(ptr::null_mut())
}

pub(crate) fn fixed_string(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

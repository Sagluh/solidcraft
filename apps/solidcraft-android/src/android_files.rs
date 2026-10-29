//! Android Storage Access Framework host services for the native CAD front end.
//! A tiny Java NativeActivity subclass launches the system document chooser,
//! copies imports into app-private storage, and exports finished native saves.
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::SystemTime;

use android_activity::AndroidApp;
use jni::{JavaVM, JValue, jni_sig, jni_str};
use jni::objects::{JObject, JString};

use solidcraft_ui_egui::{FilePickerFuture, Services, SolveApp};

const CANCELLED: &str = "__SOLIDCRAFT_CANCELLED__";

// Borrow the Java NativeActivity from android-activity without taking ownership
// of its global reference, which remains owned by the platform.
fn launch(app: &AndroidApp, mode: i32, suggested_name: &str) -> Result<(), String> {
    // SAFETY: android-activity supplies the live JavaVM and global Activity
    // reference for the lifetime of AndroidApp.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        let ptr = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: this is android-activity's borrowed global JNI reference.
        let activity = unsafe { env.as_cast_raw::<JObject>(&ptr)? };
        let title = env.new_string(suggested_name)?;
        env.call_method(
            &*activity,
            jni_str!("launchFilePicker"),
            jni_sig!("(ILjava/lang/String;)V"),
            &[JValue::Int(mode), JValue::Object(&title)],
        )?;
        Ok(())
    }).map_err(|e| format!("Android document picker: {e}"))
}

fn result(app: &AndroidApp) -> Result<String, String> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| -> jni::errors::Result<String> {
        let ptr = app.activity_as_ptr() as jni::sys::jobject;
        let activity = unsafe { env.as_cast_raw::<JObject>(&ptr)? };
        let value = env.call_method(
            &*activity,
            jni_str!("takeFilePickerResult"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?.l()?;
        let string = JString::cast_local(env, value)?;
        string.try_to_string(env)
    }).map_err(|e| format!("Android document result: {e}"))
}

fn pending_export(app: &AndroidApp) -> Result<String, String> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| -> jni::errors::Result<String> {
        let ptr = app.activity_as_ptr() as jni::sys::jobject;
        let activity = unsafe { env.as_cast_raw::<JObject>(&ptr)? };
        let value = env.call_method(
            &*activity,
            jni_str!("getPendingSavePath"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?.l()?;
        let string = JString::cast_local(env, value)?;
        string.try_to_string(env)
    }).map_err(|e| format!("Android export path: {e}"))
}

fn copy_export(app: &AndroidApp, path: &str) -> Result<bool, String> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| -> jni::errors::Result<bool> {
        let ptr = app.activity_as_ptr() as jni::sys::jobject;
        let activity = unsafe { env.as_cast_raw::<JObject>(&ptr)? };
        let path_j = env.new_string(path)?;
        env.call_method(
            &*activity,
            jni_str!("commitSavedFile"),
            jni_sig!("(Ljava/lang/String;)Z"),
            &[JValue::Object(&path_j)],
        )?.z()
    }).map_err(|e| format!("Android export: {e}"))
}

// The existing CAD file picker polls again every 50ms and requests a repaint,
// so this future does not block the renderer or busy-loop the Java UI thread.
struct Picker {
    app: AndroidApp,
}

impl Future for Picker {
    type Output = Option<String>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match result(&self.app) {
            Ok(value) if value.is_empty() => Poll::Pending,
            Ok(value) if value == CANCELLED => Poll::Ready(None),
            Ok(value) => Poll::Ready(Some(value)),
            Err(err) => {
                log::error!("{err}");
                Poll::Ready(None)
            }
        }
    }
}

fn picker(app: AndroidApp, mode: i32, name: String) -> FilePickerFuture {
    if let Err(err) = launch(&app, mode, &name) {
        log::error!("{err}");
        return Box::pin(std::future::ready(None));
    }
    Box::pin(Picker { app })
}

/// Resolve the private app storage folder from NativeActivity rather than
/// relying on HOME or XDG_DATA_HOME, which Android does not provide.
/// The engine's recovery system writes atomic .solidcraft checkpoints here.
pub fn recovery_dir(app: &AndroidApp) -> Result<std::path::PathBuf, String> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let root = vm.attach_current_thread(|env| -> jni::errors::Result<String> {
        let ptr = app.activity_as_ptr() as jni::sys::jobject;
        let activity = unsafe { env.as_cast_raw::<JObject>(&ptr)? };
        let directory = env.call_method(
            &*activity,
            jni_str!("getFilesDir"),
            jni_sig!("()Ljava/io/File;"),
            &[],
        )?.l()?;
        let path = env.call_method(
            &directory,
            jni_str!("getAbsolutePath"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?.l()?;
        JString::cast_local(env, path)?.try_to_string(env)
    }).map_err(|e| format!("Android autosave directory: {e}"))?;
    if root.is_empty() {
        return Err("Android returned an empty app storage path".into());
    }
    Ok(std::path::PathBuf::from(root).join("solidcraft-recovery"))
}

// Connect File, Import, Save As and Export commands to Android's SAF.
// The CAD engine sees local paths while Android mediates external URIs.
pub fn services(app: &AndroidApp) -> Services {
    let open = app.clone();
    let save = app.clone();
    let file = app.clone();
    Services {
        pick_open: Some(Box::new(move |_| picker(open.clone(), 1, String::new()))),
        pick_save: Some(Box::new(move |name, _| picker(save.clone(), 2, name.to_string()))),
        pick_file: Some(Box::new(move |_, _| picker(file.clone(), 1, String::new()))),
        ..Services::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    path: String,
    length: u64,
    modified: Option<SystemTime>,
}

// Observe completed saves and re-saves. The engine writes into private storage.
// Once its worker finishes and size/time are stable across two frames, copy to SAF.
pub struct AndroidFiles {
    app: AndroidApp,
    previous: Option<Stamp>,
    copied: Option<Stamp>,
}

impl AndroidFiles {
    pub fn new(app: AndroidApp) -> Self {
        Self { app, previous: None, copied: None }
    }

    pub fn tick(&mut self, cad: &mut SolveApp) {
        if cad.file_picker_busy() {
            self.previous = None;
            return;
        }
        let path = match pending_export(&self.app) {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => return,
            Err(error) => {
                log::warn!("{error}");
                return;
            }
        };
        let Ok(metadata) = std::fs::metadata(&path) else {
            self.previous = None;
            return;
        };
        let stamp = Stamp { path: path.clone(), length: metadata.len(), modified: metadata.modified().ok() };
        if self.previous.as_ref() == Some(&stamp) && self.copied.as_ref() != Some(&stamp) {
            match copy_export(&self.app, &path) {
                Ok(true) => {
                    self.copied = Some(stamp.clone());
                    cad.set_status("Файл сохранён через Android", false);
                }
                Ok(false) => {
                    self.copied = Some(stamp.clone());
                    cad.set_status("Android не смог записать файл. Повторите сохранение.", true);
                }
                Err(error) => {
                    self.copied = Some(stamp.clone());
                    cad.set_status(error, true);
                }
            }
        }
        self.previous = Some(stamp);
    }
}

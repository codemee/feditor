//! macOS：接收 Finder 的「打開方式」／把檔案拖到 Dock 圖示上的「開啟文件」事件。
//!
//! Finder 開檔案不是用命令列參數，而是送 Apple Event 給程式。NSApplication 收到之後會呼叫它的 delegate 的
//! `application:openFiles:`，但 winit 的 delegate 沒有這個方法，結果就是「FEditor cannot open files of this type」。
//! 所以這裡在 delegate 的類別上補上這個方法（執行階段 `class_addMethod`），把收到的路徑放進全域佇列，
//! 由主畫面迴圈用 [`take`] 取走。冷啟動（用 Finder 開檔案而啟動）與程式已在執行時都走這條路。
//!
//! 補方法的時機：winit 在事件迴圈開始前才設定 delegate，而 NSApplication 要到 `finishLaunching` 才會把開啟文件的
//! 事件送給 delegate，所以在 `NSApplicationWillFinishLaunchingNotification` 時補（此時 delegate 已經設好、事件還沒送）。

use std::path::PathBuf;
use std::sync::Mutex;

use objc2::ffi::class_addMethod;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject, Sel};
use objc2::{class, define_class, msg_send, sel, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSString, NSURL};

/// 收到、還沒被主畫面取走的路徑，以及收到時要叫醒畫面的函式。
static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static WAKE: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

/// 取走目前收到的路徑。
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()))
}

/// 設定收到路徑時叫醒畫面的函式（畫面建好之後才有 egui 的 Context）。
pub fn set_wake(wake: impl Fn() + Send + 'static) {
    *WAKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(wake));
}

/// delegate 上補的 `application:openFiles:`。
extern "C" fn open_files(_this: *mut AnyObject, _cmd: Sel, app: *mut AnyObject, files: *mut AnyObject) {
    // SAFETY: files 是 NSArray<NSString *>；app 是 NSApplication。
    unsafe {
        let n: usize = msg_send![files, count];
        let mut paths = Vec::new();
        for i in 0..n {
            let s: Option<Retained<NSString>> = msg_send![files, objectAtIndex: i];
            if let Some(s) = s {
                paths.push(PathBuf::from(s.to_string()));
            }
        }
        if !paths.is_empty() {
            PENDING.lock().unwrap_or_else(|e| e.into_inner()).extend(paths);
            if let Some(wake) = WAKE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                wake();
            }
        }
        // NSApplicationDelegateReplySuccess = 0：告訴系統開啟文件的事件處理完了
        let _: () = msg_send![app, replyToOpenOrPrint: 0usize];
    }
}

/// 在目前的 NSApplication delegate 的類別上補 `application:openFiles:`（已經有就不動）。
fn patch_delegate() {
    // SAFETY: 標準的 Objective-C 執行階段呼叫；型別編碼 "v@:@@" 與 open_files 的簽章相符。
    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let delegate: Option<Retained<AnyObject>> = msg_send![&*app, delegate];
        let Some(delegate) = delegate else { return };
        let cls: &AnyClass = delegate.class();
        let sel = sel!(application:openFiles:);
        if cls.instance_method(sel).is_some() {
            return;
        }
        let imp: objc2::runtime::Imp = std::mem::transmute(open_files as extern "C" fn(_, _, _, _));
        class_addMethod(
            cls as *const AnyClass as *mut AnyClass,
            sel,
            imp,
            c"v@:@@".as_ptr(),
        );
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FEditorLaunchObserver"]
    struct Observer;

    impl Observer {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _note: &AnyObject) {
            patch_delegate();
        }
    }
);

/// 安裝。要在主執行緒、事件迴圈開始之前呼叫。
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let observer: Retained<Observer> = unsafe { msg_send![Observer::alloc(mtm), init] };
    // SAFETY: 通知中心不持有觀察者，所以下面讓它活到程式結束；選擇器與方法相符。
    unsafe {
        let center: Retained<AnyObject> = msg_send![class!(NSNotificationCenter), defaultCenter];
        let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
        let _: () = msg_send![
            &*center,
            addObserver: &*observer,
            selector: sel!(willFinishLaunching:),
            name: &*name,
            object: std::ptr::null::<AnyObject>()
        ];
    }
    std::mem::forget(observer);
}

use std::sync::Mutex;

use objc2::{
    ClassType, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject},
    sel,
};
use objc2_foundation::{NSAppleEventDescriptor, NSNotificationCenter, NSString};
use winit::event_loop::EventLoopProxy;

use super::AppEvent;

const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const WILL_FINISH_LAUNCHING: &str = "NSApplicationWillFinishLaunchingNotification";

static SINK: Mutex<Option<EventLoopProxy<AppEvent>>> = Mutex::new(None);

fn file_path(descriptor: &NSAppleEventDescriptor) -> Option<String> {
    let url = descriptor.fileURLValue()?;
    Some(url.path()?.to_string())
}

fn opened_paths(event: &NSAppleEventDescriptor) -> Vec<String> {
    // SAFETY: `paramDescriptorForKeyword:` takes a four-character code and returns an optional,
    // autoreleased descriptor owned by the event.
    let list: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
    let Some(list) = list else {
        return Vec::new();
    };
    match list.numberOfItems() {
        0 => file_path(&list).into_iter().collect(),
        count => (1..=count)
            .filter_map(|index| list.descriptorAtIndex(index))
            .filter_map(|item| file_path(&item))
            .collect(),
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and the class implements no Drop.
    #[unsafe(super(NSObject))]
    #[name = "VeloceDocumentHandler"]
    struct DocumentHandler;

    impl DocumentHandler {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _notification: &AnyObject) {
            let manager: Retained<AnyObject> =
                // SAFETY: the shared manager is a process-wide singleton.
                unsafe { msg_send![objc2::class!(NSAppleEventManager), sharedAppleEventManager] };
            // SAFETY: the handler outlives the process and the selector matches the signature
            // `handleOpenDocuments:withReplyEvent:` defined below.
            unsafe {
                let _: () = msg_send![
                    &*manager,
                    setEventHandler: self,
                    andSelector: sel!(handleOpenDocuments:withReplyEvent:),
                    forEventClass: CORE_EVENT_CLASS,
                    andEventID: OPEN_DOCUMENTS
                ];
            }
        }

        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn handle_open_documents(&self, event: &NSAppleEventDescriptor, _reply: &AnyObject) {
            let paths = opened_paths(event);
            if paths.is_empty() {
                return;
            }
            if let Ok(sink) = SINK.lock()
                && let Some(proxy) = sink.as_ref()
            {
                let _ = proxy.send_event(AppEvent::Open(paths));
            }
        }
    }
);

pub fn install_document_handler(proxy: EventLoopProxy<AppEvent>) {
    if let Ok(mut sink) = SINK.lock() {
        *sink = Some(proxy);
    }
    // SAFETY: `new` returns a retained, initialized NSObject subclass instance.
    let handler: Retained<DocumentHandler> = unsafe { msg_send![DocumentHandler::class(), new] };
    let name = NSString::from_str(WILL_FINISH_LAUNCHING);
    // SAFETY: the observer is leaked below so it stays valid for the lifetime of the process, and
    // the selector takes a single notification argument.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &handler,
            sel!(willFinishLaunching:),
            Some(&name),
            None,
        );
    }
    std::mem::forget(handler);
}
